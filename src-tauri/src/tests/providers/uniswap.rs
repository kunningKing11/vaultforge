use serde_json::{Value, json};

use super::{
    DEFAULT_SLIPPAGE_BPS, UniswapQuoteRequest, estimate_routing_quote_fee, minimum_output,
    verify_routing_quote,
};
use crate::providers::cctp::tests::MockRpc;
use crate::registry::network_by_id;
use crate::uniswap_config::deployment_for_network;

const INPUT: &str = "0x1111111111111111111111111111111111111111";
const OUTPUT: &str = "0x2222222222222222222222222222222222222222";
const SENDER: &str = "0x3333333333333333333333333333333333333333";
const NOW: u64 = 1_800_000_000;

fn request() -> UniswapQuoteRequest<'static> {
    UniswapQuoteRequest {
        network_id: "base",
        token_in: INPUT,
        token_out: OUTPUT,
        sender: SENDER,
        amount: 1_000_000,
    }
}

fn fixture() -> (Value, Value) {
    // Independent ABI fixture from IV3SwapRouter and IMulticallExtended:
    // https://github.com/Uniswap/swap-router-contracts/tree/main/contracts/interfaces
    // Literal selectors; ABI words formatted here without production encoders.
    let inner = format!(
        "04e45aaf{:0>64}{:0>64}{:064x}{:0>64}{:064x}{:064x}{:064x}",
        &INPUT[2..],
        &OUTPUT[2..],
        3000,
        &SENDER[2..],
        1_000_000,
        1_990_000,
        0,
    );
    let data = format!(
        "0x5ae401dc{:064x}{:064x}{:064x}{:064x}{:064x}{inner}{}",
        NOW + 600,
        64,
        1,
        32,
        228,
        "00".repeat(28),
    );
    assert_eq!(data.len(), 842);
    (
        json!({"routing":"CLASSIC", "quote": {
            "chainId":8453,
            "input":{"token":INPUT, "amount":"1000000"},
            "output":{"token":OUTPUT, "amount":"2000000"},
        }}),
        json!({"swap": {
            "chainId":8453, "from":SENDER,
            "to":deployment_for_network("base").unwrap().router,
            "value":"0", "data":data,
            // Deliberately wrong API fees: live estimation must ignore them.
            "gasLimit":"1", "maxFeePerGas":"1",
        }}),
    )
}

fn replace_bytes(swap: &mut Value, offset: usize, bytes: &[u8]) {
    let mut data = hex::decode(
        swap["swap"]["data"]
            .as_str()
            .unwrap()
            .trim_start_matches("0x"),
    )
    .unwrap();
    data[offset..offset + bytes.len()].copy_from_slice(bytes);
    swap["swap"]["data"] = json!(format!("0x{}", hex::encode(data)));
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn accepts_only_the_requested_deadline_wrapped_swap() {
    let (quote, mut swap) = fixture();
    let verified = verify_routing_quote(&request(), &quote, &swap, None, NOW).unwrap();
    assert_eq!(verified.minimum_received(), 1_990_000);
    assert_eq!(verified.quoted_output(), 2_000_000);
    assert_eq!(verified.deadline(), NOW + 600);
    assert_eq!(verified.call().value, 0);
    assert_eq!(verified.call().data.len(), 420);
    // EVM address identity is case-insensitive, including the allowlisted router.
    swap["swap"]["to"] = json!(
        deployment_for_network("base")
            .unwrap()
            .router
            .to_lowercase()
    );
    assert!(verify_routing_quote(&request(), &quote, &swap, Some(50), NOW).is_ok());
}

#[test]
fn calculates_minimum_output_with_checked_integer_math() {
    assert_eq!(DEFAULT_SLIPPAGE_BPS, 50);
    for (amount, bps, expected) in [
        (100, 0, 100),
        (10_000, 50, 9950),
        (101, 50, 100),
        (100, 10_000, 0),
        (0, 50, 0),
        (u128::MAX, 10_000, 0),
    ] {
        assert_eq!(minimum_output(amount, bps).unwrap(), expected);
    }
    assert!(minimum_output(100, 10_001).is_err());
    assert!(minimum_output(100, u16::MAX).is_err());
    assert!(minimum_output(u128::MAX, 0).is_err());
    assert!(minimum_output(u128::MAX, 50).is_err());
}

#[test]
fn chooses_the_best_supported_on_chain_pool_without_an_api_key() {
    use crate::uniswap::{SwapIntent, SwapProvider};
    let server = MockRpc::new(|request| {
        if request["method"] == "eth_chainId" {
            return json!({"jsonrpc":"2.0","id":1,"result":"0x2105"});
        }
        assert_eq!(request["method"], "eth_call");
        assert_eq!(
            request["params"][0]["to"],
            deployment_for_network("base").unwrap().quoter_v2
        );
        let bytes = hex::decode(
            request["params"][0]["data"]
                .as_str()
                .unwrap()
                .trim_start_matches("0x"),
        )
        .unwrap();
        assert_eq!(hex::encode(&bytes[..4]), "c6a5026a");
        assert_eq!(bytes.len(), 164);
        let fee = u32::from_be_bytes(bytes[128..132].try_into().unwrap());
        if fee == 100 || fee == 10_000 {
            return json!({"jsonrpc":"2.0","id":1,"error":{"code":3,"message":"execution reverted"}});
        }
        let output = if fee == 3000 { 2_000_000 } else { 1_900_000 };
        json!({"jsonrpc":"2.0","id":1,"result":format!("0x{output:064x}{:064x}{:064x}{:064x}",1,0,100_000)})
    });
    let mut network = network_by_id("base").unwrap().clone();
    network.rpc_url = Some(server.url.clone());
    let intent = SwapIntent {
        api: None,
        relay: None,
        network,
        token_in: INPUT.into(),
        token_out: OUTPUT.into(),
        sender: SENDER.into(),
        amount: 1_000_000,
    };
    let client = reqwest::Client::new();
    let provider = super::LiveSwapProvider { client: &client };
    let quote = runtime().block_on(provider.quote(&intent, None)).unwrap();
    assert_eq!(quote.pool_fee, 3000);
    assert_eq!(quote.amount_out, 2_000_000);
    let quote = runtime()
        .block_on(provider.quote(&intent, Some(500)))
        .unwrap();
    assert_eq!(quote.pool_fee, 500);
    assert_eq!(quote.amount_out, 1_900_000);
}

#[test]
fn estimates_live_fees_for_the_verified_call_with_headroom() {
    let (quote, swap) = fixture();
    let verified = verify_routing_quote(&request(), &quote, &swap, None, NOW).unwrap();
    let expected_data = swap["swap"]["data"].clone();
    let expected_router = swap["swap"]["to"].clone();
    let server = MockRpc::new(move |request| {
        let result = match request["method"].as_str().unwrap() {
            "eth_chainId" => json!("0x2105"),
            "eth_feeHistory" => json!({"baseFeePerGas":["0x64"], "reward":[["0xa"]]}),
            "eth_estimateGas" => {
                assert_eq!(request["params"][0]["data"], expected_data);
                assert_eq!(request["params"][0]["to"], expected_router);
                assert_eq!(request["params"][0]["from"], SENDER);
                assert_eq!(request["params"][0]["value"], "0x0");
                json!("0x5208")
            }
            method => panic!("unexpected RPC method {method}"),
        };
        json!({"jsonrpc":"2.0", "id":1, "result":result})
    });
    let mut network = network_by_id("base").unwrap().clone();
    network.rpc_url = Some(server.url.clone());
    let estimate = runtime()
        .block_on(estimate_routing_quote_fee(
            &reqwest::Client::new(),
            &network,
            &verified,
            NOW,
        ))
        .unwrap();
    assert_eq!(estimate.gas_limit, 21_000);
    assert_eq!(estimate.fees.max_priority_fee_per_gas, 10);
    assert_eq!(estimate.fees.max_fee_per_gas, 242); // ceil((2 * 100 + 10) * 1.15)
    assert_eq!(estimate.max_native_fee, 21_000 * 242);
}

#[test]
fn rejects_expired_and_overlong_deadlines() {
    for deadline in [NOW - 1, NOW, NOW + 1201, u64::MAX] {
        let (quote, mut swap) = fixture();
        replace_bytes(&mut swap, 28, &deadline.to_be_bytes());
        assert!(verify_routing_quote(&request(), &quote, &swap, None, NOW).is_err());
    }
    for deadline in [NOW + 1, NOW + 1200] {
        let (quote, mut swap) = fixture();
        replace_bytes(&mut swap, 28, &deadline.to_be_bytes());
        assert!(verify_routing_quote(&request(), &quote, &swap, None, NOW).is_ok());
    }
}

#[test]
fn rejects_fee_provider_mismatch_and_expiry_before_rpc() {
    let (quote, swap) = fixture();
    let verified = verify_routing_quote(&request(), &quote, &swap, None, NOW).unwrap();
    let mut network = network_by_id("base").unwrap().clone();
    network.rpc_url = Some("http://127.0.0.1:1".into());
    let client = reqwest::Client::new();
    let runtime = runtime();
    let error = runtime
        .block_on(estimate_routing_quote_fee(
            &client,
            &network,
            &verified,
            NOW + 600,
        ))
        .unwrap_err();
    assert!(error.contains("expiry"));
    network.chain_id = Some(1);
    let error = runtime
        .block_on(estimate_routing_quote_fee(
            &client, &network, &verified, NOW,
        ))
        .unwrap_err();
    assert!(error.contains("network"));
}

#[test]
fn rejects_invalid_quoter_results_and_non_pool_provider_failures() {
    use crate::uniswap::{SwapIntent, SwapProvider};
    for result in [
        "0x".into(),
        "0xzz".into(),
        format!("0x{}", "00".repeat(128)),
        format!("0x01{}", "00".repeat(127)),
    ] {
        assert!(super::parse_pool_quote(&result).is_err());
    }
    let server = MockRpc::new(|request| {
        if request["method"] == "eth_chainId" {
            return json!({"jsonrpc":"2.0","id":1,"result":"0x2105"});
        }
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"provider temporarily unavailable"}})
    });
    let mut network = network_by_id("base").unwrap().clone();
    network.rpc_url = Some(server.url.clone());
    let intent = SwapIntent {
        api: None,
        relay: None,
        network,
        token_in: INPUT.into(),
        token_out: OUTPUT.into(),
        sender: SENDER.into(),
        amount: 1_000_000,
    };
    let client = reqwest::Client::new();
    let error = runtime()
        .block_on(super::LiveSwapProvider { client: &client }.quote(&intent, None))
        .err()
        .unwrap();
    assert!(error.contains("provider temporarily unavailable"));
}

#[test]
fn rejects_live_chain_errors_and_invalid_fee_estimates() {
    for failure in ["chain", "gas", "fee", "overflow", "rpc"] {
        let server = MockRpc::new(move |request| {
            if failure == "rpc" && request["method"] == "eth_estimateGas" {
                return json!({"jsonrpc":"2.0", "id":1, "error":{"code":-32000,"message":"execution reverted"}});
            }
            let result = match request["method"].as_str().unwrap() {
                "eth_chainId" => json!(if failure == "chain" { "0x1" } else { "0x2105" }),
                "eth_feeHistory" if failure == "fee" => json!({}),
                "eth_feeHistory" if failure == "overflow" => json!({
                    "baseFeePerGas":["0x7fffffffffffffffffffffffffffffff"], "reward":[["0x1"]],
                }),
                "eth_feeHistory" => json!({"baseFeePerGas":["0x64"], "reward":[["0xa"]]}),
                "eth_estimateGas" => json!(if failure == "gas" { "0x0" } else { "0x1" }),
                method => panic!("unexpected RPC method {method}"),
            };
            json!({"jsonrpc":"2.0", "id":1, "result":result})
        });
        let (quote, swap) = fixture();
        let verified = verify_routing_quote(&request(), &quote, &swap, None, NOW).unwrap();
        let mut network = network_by_id("base").unwrap().clone();
        network.rpc_url = Some(server.url.clone());
        assert!(
            runtime()
                .block_on(estimate_routing_quote_fee(
                    &reqwest::Client::new(),
                    &network,
                    &verified,
                    NOW,
                ))
                .is_err(),
            "{failure}"
        );
    }
}

#[test]
fn rejects_malformed_amounts_and_unsupported_requests() {
    for value in [
        json!(null),
        json!(1),
        json!(""),
        json!("0"),
        json!("-1"),
        json!("+1"),
        json!("1.5"),
        json!(format!("{}0", u128::MAX)),
    ] {
        for side in ["input", "output"] {
            let (mut quote, swap) = fixture();
            quote["quote"][side]["amount"] = value.clone();
            assert!(verify_routing_quote(&request(), &quote, &swap, None, NOW).is_err());
        }
    }
    let (quote, swap) = fixture();
    for request in [
        UniswapQuoteRequest {
            network_id: "ethereum",
            ..request()
        },
        UniswapQuoteRequest {
            token_out: INPUT,
            ..request()
        },
        UniswapQuoteRequest {
            amount: 0,
            ..request()
        },
        UniswapQuoteRequest {
            sender: "0x0000000000000000000000000000000000000000",
            ..request()
        },
    ] {
        assert!(verify_routing_quote(&request, &quote, &swap, None, NOW).is_err());
    }
    assert!(verify_routing_quote(&request(), &quote, &swap, Some(10_001), NOW).is_err());
}

#[test]
fn rejects_missing_or_tampered_response_fields() {
    let (quote, swap) = fixture();
    for (pointer, value) in [
        ("/routing", json!("DUTCH_V2")),
        ("/quote/chainId", json!(1)),
        ("/quote/input/token", json!(OUTPUT)),
        ("/quote/output/token", json!(INPUT)),
        ("/quote/input/amount", json!("1")),
        ("/quote/output/amount", json!("1")),
    ] {
        let mut altered = quote.clone();
        *altered.pointer_mut(pointer).unwrap() = value;
        assert!(
            verify_routing_quote(&request(), &altered, &swap, None, NOW).is_err(),
            "{pointer}"
        );
        *altered.pointer_mut(pointer).unwrap() = Value::Null;
        assert!(
            verify_routing_quote(&request(), &altered, &swap, None, NOW).is_err(),
            "missing {pointer}"
        );
    }
    for (field, value) in [
        ("to", json!(INPUT)),
        ("from", json!(OUTPUT)),
        ("chainId", json!(1)),
        ("value", json!("1")),
        ("data", json!("0xzz")),
    ] {
        let mut altered = swap.clone();
        altered["swap"][field] = value;
        assert!(
            verify_routing_quote(&request(), &quote, &altered, None, NOW).is_err(),
            "{field}"
        );
    }
    for field in ["to", "from", "chainId", "value", "data"] {
        let mut altered = swap.clone();
        altered["swap"].as_object_mut().unwrap().remove(field);
        assert!(verify_routing_quote(&request(), &quote, &altered, None, NOW).is_err());
    }
}

#[test]
fn rejects_tampered_calldata_fields_and_noncanonical_wrappers() {
    let (quote, swap) = fixture();
    // Selectors, offsets, call count/length, addresses and their padding,
    // uint24 padding, amount/minimum high bits, price limit, and final padding.
    for offset in [
        0, 36, 67, 99, 100, 131, 132, 163, 164, 168, 199, 200, 231, 232, 264, 295, 296, 327, 328,
        359, 360, 391, 392, 419,
    ] {
        let mut altered = swap.clone();
        let data = hex::decode(
            swap["swap"]["data"]
                .as_str()
                .unwrap()
                .trim_start_matches("0x"),
        )
        .unwrap();
        replace_bytes(&mut altered, offset, &[data[offset] ^ 1]);
        assert!(
            verify_routing_quote(&request(), &quote, &altered, None, NOW).is_err(),
            "byte {offset}"
        );
    }
    for data in [
        "0x".to_string(),
        format!("{}00", swap["swap"]["data"].as_str().unwrap()),
        swap["swap"]["data"].as_str().unwrap()[..840].to_string(),
    ] {
        let mut altered = swap.clone();
        altered["swap"]["data"] = json!(data);
        assert!(verify_routing_quote(&request(), &quote, &altered, None, NOW).is_err());
    }
}
