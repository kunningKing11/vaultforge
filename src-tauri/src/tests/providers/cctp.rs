use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use super::{
    CctpSourcePreparation, fetch_standard_fee_at, prepare_cctp_mint,
    prepare_source_after_fee_check, validate_standard_fee,
};
use crate::cctp::{CctpEnvironment, CctpTransfer, tests::transfer};
use crate::providers::evm::{evm_max_native_fee, fetch_evm_allowance};
use crate::tx::cctp::{build_approval_call, build_burn_call, build_mint_call};

pub(crate) struct MockRpc {
    pub(crate) url: String,
    pub(crate) requests: Arc<Mutex<Vec<serde_json::Value>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl MockRpc {
    pub(crate) fn new(
        handler: impl Fn(&serde_json::Value) -> serde_json::Value + Send + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(20);
            while !stopped.load(Ordering::Relaxed) && Instant::now() < deadline {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("mock accept: {error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let (header_end, length) = loop {
                    let mut chunk = [0; 4096];
                    let read = stream.read(&mut chunk).unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&chunk[..read]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]);
                        let length = header
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|n| n.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break (end + 4, length);
                        }
                    }
                };
                let request = if length == 0 {
                    serde_json::Value::Null
                } else {
                    serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap()
                };
                recorded.lock().unwrap().push(request.clone());
                let body = handler(&request).to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for MockRpc {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn rpc(
    chain: u64,
    balance: u128,
    usdc: u128,
    allowance: &str,
    expected_call: Option<Vec<u8>>,
) -> MockRpc {
    let allowance = allowance.to_string();
    MockRpc::new(move |request| {
        let result = match request["method"].as_str().unwrap() {
            "eth_chainId" => serde_json::json!(format!("0x{chain:x}")),
            "eth_getBalance" => {
                assert_eq!(
                    request["params"][0],
                    "0x3333333333333333333333333333333333333333"
                );
                serde_json::json!(format!("0x{balance:x}"))
            }
            "eth_call" => {
                let data = request["params"][0]["data"].as_str().unwrap();
                assert_eq!(request["params"][1], "latest");
                if data.starts_with("0x70a08231") {
                    serde_json::json!(format!("0x{usdc:064x}"))
                } else {
                    assert!(data.starts_with("0xdd62ed3e"));
                    assert_eq!(data.len(), 138);
                    serde_json::json!(allowance)
                }
            }
            "eth_feeHistory" => serde_json::json!({"baseFeePerGas":["0x1"], "reward":[["0x1"]]}),
            "eth_estimateGas" => {
                assert_eq!(
                    request["params"][0]["from"],
                    "0x3333333333333333333333333333333333333333"
                );
                let actual = request["params"][0]["data"].as_str().unwrap();
                assert_eq!(
                    actual,
                    format!(
                        "0x{}",
                        hex::encode(expected_call.as_ref().expect("unexpected gas estimation"))
                    )
                );
                assert_eq!(request["params"][0]["value"], "0x0");
                serde_json::json!("0x64")
            }
            method => panic!("Unexpected RPC method {method}"),
        };
        serde_json::json!({"jsonrpc":"2.0", "id":1, "result":result})
    })
}

fn mocked_transfer(source: &MockRpc, destination: &MockRpc) -> CctpTransfer {
    let mut t = transfer(CctpEnvironment::Mainnet);
    t.route.source.network.rpc_url = Some(source.url.clone());
    t.route.destination.network.rpc_url = Some(destination.url.clone());
    t
}

#[test]
fn estimates_real_mint_calldata_and_rechecks_destination_gas() {
    let template = transfer(CctpEnvironment::Mainnet);
    let call = build_mint_call(&template, &[1; 33], &[2; 65]).unwrap();
    for balance in [299, 300] {
        let source = rpc(1, 1_000_000, 1_000_000, &format!("0x{:064x}", 0), None);
        let destination = rpc(42161, balance, 0, "", Some(call.data.clone()));
        let t = mocked_transfer(&source, &destination);
        let result = runtime().block_on(prepare_cctp_mint(
            &reqwest::Client::new(),
            &t,
            &[1; 33],
            &[2; 65],
        ));
        assert_eq!(result.is_ok(), balance == 300);
        if let Ok(prepared) = result {
            assert_eq!(prepared.estimate.max_native_fee, 300);
        }
    }
}

#[test]
fn fetches_live_standard_fee_and_rejects_nonzero_or_malformed_quotes() {
    for json in [
        serde_json::json!([{ "finalityThreshold":1000,"minimumFee":1 }, { "finalityThreshold":2000,"minimumFee":0 }]),
        serde_json::json!([{ "finalityThreshold":2000,"minimumFee":0.1 }]),
        serde_json::json!([{ "finalityThreshold":2000,"minimumFee":"0" }]),
        serde_json::json!([{ "finalityThreshold":1000,"minimumFee":0 }]),
        serde_json::json!([{ "finalityThreshold":2000,"minimumFee":0 }, { "finalityThreshold":2000,"minimumFee":0 }]),
        serde_json::json!({"error":"unavailable"}),
    ] {
        let expected = validate_standard_fee(&json).is_ok();
        let server = MockRpc::new(move |_| json.clone());
        let result =
            runtime().block_on(fetch_standard_fee_at(&reqwest::Client::new(), &server.url));
        assert_eq!(result.is_ok(), expected);
    }
}

#[test]
fn parses_full_uint256_allowance_and_rejects_invalid_rpc_results() {
    let t = transfer(CctpEnvironment::Mainnet);
    for value in [
        format!("0x{:064x}", 999999),
        format!("0x{:064x}", 1000000),
        format!("0x{}", "ff".repeat(32)),
        "0x1".into(),
        "0xnothex".into(),
    ] {
        let server = rpc(1, 0, 0, &value, None);
        let mut config = t.route.source.network.clone();
        config.rpc_url = Some(server.url.clone());
        let result = runtime().block_on(fetch_evm_allowance(
            &reqwest::Client::new(),
            &config,
            t.route.source.usdc,
            t.sender(),
            t.route.source.token_messenger,
        ));
        if value.len() != 66 {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap().covers(1000000), !value.ends_with("0f423f"));
        }
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests[0]["params"][0]["to"], t.route.source.usdc);
        assert_eq!(
            requests[0]["params"][0]["data"],
            format!(
                "0xdd62ed3e{}{}{}{}",
                "00".repeat(12),
                "33".repeat(20),
                "00".repeat(12),
                &t.route.source.token_messenger[2..].to_lowercase()
            )
        );
    }
}

#[test]
fn prepares_approval_budget_without_estimating_burn_before_allowance() {
    let template = transfer(CctpEnvironment::Mainnet);
    let source = rpc(
        1,
        1_000_000,
        1_000_000,
        &format!("0x{:064x}", 999999),
        Some(build_approval_call(&template).unwrap().data),
    );
    let destination = rpc(42161, 1_000_000, 0, "", None);
    let t = mocked_transfer(&source, &destination);
    let quote = runtime()
        .block_on(prepare_source_after_fee_check(&reqwest::Client::new(), &t))
        .unwrap();
    assert_eq!(quote.source_native_budget, 600300);
    assert_eq!(quote.provisional_destination_fee, 900000);
    assert_eq!(quote.protocol_fee, 0);
    assert_eq!(quote.expected_receive_amount, 1000000);
    let CctpSourcePreparation::Approval {
        approval,
        provisional_burn_fee,
    } = quote.source
    else {
        panic!("Expected approval")
    };
    assert_eq!(approval.call.to, t.route.source.usdc);
    assert_eq!(provisional_burn_fee, 600000);
}

#[test]
fn propagates_rpc_errors_without_preparing_source_or_mint_calls() {
    let source = MockRpc::new(|request| {
        if request["method"] == "eth_chainId" {
            serde_json::json!({"result":"0x1"})
        } else {
            serde_json::json!({"error":{"code":-32000,"message":"provider unavailable"}})
        }
    });
    let destination = MockRpc::new(|request| match request["method"].as_str().unwrap() {
        "eth_chainId" => serde_json::json!({"result":"0xa4b1"}),
        "eth_feeHistory" => {
            serde_json::json!({"result":{"baseFeePerGas":["0x1"],"reward":[["0x1"]]}})
        }
        "eth_estimateGas" => {
            serde_json::json!({"error":{"code":-32000,"message":"invalid attestation"}})
        }
        method => panic!("Unexpected method {method}"),
    });
    let t = mocked_transfer(&source, &destination);
    let client = reqwest::Client::new();
    let source_error = runtime()
        .block_on(prepare_source_after_fee_check(&client, &t))
        .unwrap_err();
    assert!(source_error.contains("provider unavailable"));
    let mint_error = runtime()
        .block_on(prepare_cctp_mint(&client, &t, &[1; 33], &[2; 65]))
        .unwrap_err();
    assert!(mint_error.contains("invalid attestation"));
    assert!(
        source
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|request| request["method"] != "eth_estimateGas")
    );
}

#[test]
fn rejects_chain_mismatch_usdc_shortfall_and_combined_gas_shortfalls() {
    let template = transfer(CctpEnvironment::Mainnet);
    for (chain, source_balance, usdc, destination_balance) in [
        (11155111, 1_000_000, 1_000_000, 1_000_000),
        (1, 1_000_000, 999999, 1_000_000),
        (1, 300, 1_000_000, 1_000_000), // enough approval gas, not approval + burn
        (1, 1_000_000, 1_000_000, 899999),
    ] {
        let source = rpc(
            chain,
            source_balance,
            usdc,
            &format!("0x{:064x}", 0),
            Some(build_approval_call(&template).unwrap().data),
        );
        let destination = rpc(42161, destination_balance, 0, "", None);
        let t = mocked_transfer(&source, &destination);
        assert!(
            runtime()
                .block_on(prepare_source_after_fee_check(&reqwest::Client::new(), &t))
                .is_err()
        );
    }
    assert!(evm_max_native_fee(u64::MAX, u128::MAX).is_err());
    assert!(evm_max_native_fee(0, 1).is_err());
}

#[test]
fn skips_approval_and_estimates_burn_when_allowance_covers() {
    let template = transfer(CctpEnvironment::Mainnet);
    for allowance in [
        format!("0x{:064x}", 1000000),
        format!("0x{}", "ff".repeat(32)),
    ] {
        let source = rpc(
            1,
            1_000_000,
            1_000_000,
            &allowance,
            Some(build_burn_call(&template).unwrap().data),
        );
        let destination = rpc(42161, 1_000_000, 0, "", None);
        let t = mocked_transfer(&source, &destination);
        let quote = runtime()
            .block_on(prepare_source_after_fee_check(&reqwest::Client::new(), &t))
            .unwrap();
        assert_eq!(quote.source_native_budget, 300);
        let CctpSourcePreparation::Burn(burn) = quote.source else {
            panic!("Expected burn")
        };
        assert_eq!(burn.call.to, t.route.source.token_messenger);
    }
}
