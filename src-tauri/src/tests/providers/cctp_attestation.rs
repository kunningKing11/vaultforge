use super::*;
use crate::cctp_attestation::tests::{attested, operation};
use crate::providers::cctp::tests::MockRpc;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

fn response_server(status: u16, body: String, extra: &str) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let extra = extra.to_string();
    let thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0; 4096];
        let length = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..length]);
        assert!(request.contains("transactionHash=0x"));
        write!(stream,"HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n{extra}\r\n{body}",body.len()).unwrap();
    });
    (url, thread)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn handles_api_pending_rate_limits_complete_errors_and_size_limits() {
    for (status, body, extra) in [
        (404, "{}".into(), ""),
        (429, "{}".into(), "Retry-After: 300\r\n"),
        (200, "{\"messages\":[]}".into(), ""),
        (503, "{}".into(), ""),
        (200, "invalid".into(), ""),
        (200, " ".repeat(MAX_API_BYTES + 1), ""),
    ] {
        let (url, server) = response_server(status, body.clone(), extra);
        let result = runtime().block_on(fetch_messages_at(
            &reqwest::Client::new(),
            &url,
            &format!("0x{}", "ab".repeat(32)),
        ));
        server.join().unwrap();
        match status {
            404 => assert!(matches!(result.unwrap(), MessageResponse::Pending)),
            429 => assert!(matches!(result.unwrap(), MessageResponse::RateLimited(300))),
            200 if body == "{\"messages\":[]}" => {
                assert!(matches!(result.unwrap(), MessageResponse::Messages(_)))
            }
            _ => assert!(result.is_err()),
        }
    }
}

#[test]
fn rejects_invalid_attestation_rpc_and_false_simulation_results() {
    let operation = operation();
    let message = attested(&operation);
    for case in 0..3 {
        let server = MockRpc::new(move |request| match request["method"].as_str().unwrap() {
            "eth_chainId" => serde_json::json!({"result":"0xa4b1"}),
            "eth_call" if case == 0 => {
                serde_json::json!({"result":format!("0x{}",hex::encode(evm_uint_word(1)))})
            }
            "eth_call" => {
                serde_json::json!({"error":{"code":-32000,"message":"invalid attestation"}})
            }
            "eth_call" => {
                serde_json::json!({"result":format!("0x{}",hex::encode(evm_uint_word(0)))})
            }
            method => panic!("Unexpected method {method}"),
        });
        let mut transfer = operation.cctp_transfer().unwrap();
        transfer.route.destination.network.rpc_url = Some(server.url.clone());
        let result = runtime().block_on(simulate_message(
            &reqwest::Client::new(),
            &transfer,
            &message,
            &[1; 65],
        ));
        assert_eq!(result.is_ok(), case == 0);
        let requests = server.requests.lock().unwrap();
        assert_eq!(
            requests[1]["params"][0]["from"],
            transfer.destination_caller()
        );
        assert_eq!(
            requests[1]["params"][0]["to"],
            transfer.route.destination.message_transmitter
        );
        assert!(
            requests
                .iter()
                .all(|request| request["method"] != "eth_sendRawTransaction")
        );
    }
}
