use super::{fallback_rpc_urls, json_rpc_result, resilient_get, rpc_post_urls};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Serves exactly one dropped connection (a transient transport failure),
/// then records the retried request and answers 200. Returns the URL and the
/// raw requests seen after the drop.
pub(crate) fn flaky_server() -> (String, Arc<Mutex<Vec<String>>>, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/v2/burn/USDC/fees/6/1",
        listener.local_addr().unwrap()
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let handle = std::thread::spawn(move || {
        let mut dropped = false;
        for stream in listener.incoming().take(3) {
            let mut stream = stream.unwrap();
            if !dropped {
                dropped = true;
                continue;
            }
            let mut buffer = vec![0u8; 4096];
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let read = stream.read(&mut buffer).unwrap_or(0);
            recorded
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buffer[..read]).into_owned());
            let body = "{}";
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
            return;
        }
    });
    (url, seen, handle)
}

#[test]
fn fallback_lists_use_https() {
    for network in [
        "ethereum",
        "avalanche_c",
        "optimism",
        "arbitrum_one",
        "base",
        "polygon",
        "monad",
    ] {
        for url in fallback_rpc_urls(network) {
            assert!(
                url.starts_with("https://"),
                "{network} fallback must be HTTPS: {url}"
            );
        }
    }
    assert!(fallback_rpc_urls("unknown").is_empty());
}

#[test]
fn falls_back_to_secondary_endpoints_when_primary_fails() {
    let (primary, primary_hits, primary_handle) = status_server(500, "{}", 3);
    let (fallback, fallback_hits, fallback_handle) = status_server(200, "{\"ok\":true}", 1);
    let result = runtime()
        .block_on(rpc_post_urls(
            &reqwest::Client::new(),
            &[primary, fallback],
            &post_body(),
        ))
        .unwrap();
    assert_eq!(result, serde_json::json!({"ok": true}));
    assert_eq!(*primary_hits.lock().unwrap(), 3);
    assert_eq!(*fallback_hits.lock().unwrap(), 1);
    primary_handle.join().unwrap();
    fallback_handle.join().unwrap();
}

#[test]
fn keeps_primary_result_when_healthy() {
    let (primary, _, primary_handle) = status_server(200, "{\"ok\":true}", 1);
    let (_, fallback_hits, _) = status_server(500, "{}", 0);
    let result = runtime()
        .block_on(rpc_post_urls(
            &reqwest::Client::new(),
            &[primary, "http://127.0.0.1:1/unused".to_string()],
            &post_body(),
        ))
        .unwrap();
    assert_eq!(result, serde_json::json!({"ok": true}));
    assert_eq!(*fallback_hits.lock().unwrap(), 0);
    primary_handle.join().unwrap();
}

/// Serves `expect` requests with a fixed status, counting hits. `expect = 0`
/// exits immediately so untouched fallbacks never block the join.
pub(crate) fn status_server(
    status: u16,
    body: &'static str,
    expect: usize,
) -> (String, Arc<Mutex<usize>>, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(Mutex::new(0usize));
    let counted = hits.clone();
    let handle = std::thread::spawn(move || {
        for stream in listener.incoming().take(expect) {
            let mut stream = match stream {
                Ok(stream) => stream,
                Err(_) => continue,
            };
            *counted.lock().unwrap() += 1;
            let mut buffer = vec![0u8; 4096];
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let _ = stream.read(&mut buffer);
            let response = format!(
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (url, hits, handle)
}

pub(crate) fn post_body() -> serde_json::Value {
    serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "eth_chainId", "params": []})
}

#[test]
fn names_failing_host_in_rpc_errors() {
    let (url, _, handle) = status_server(500, "{}", 3);
    let error = runtime()
        .block_on(super::rpc_post(&reqwest::Client::new(), &url, &post_body()))
        .unwrap_err();
    assert!(error.contains("127.0.0.1"), "{error}");
    handle.join().unwrap();
}

#[test]
fn recovers_after_a_transient_transport_failure() {
    let (url, seen, handle) = flaky_server();
    let response = runtime()
        .block_on(resilient_get(&reqwest::Client::new(), &url, &[]))
        .unwrap();
    assert_eq!(response.status(), 200);
    handle.join().unwrap();
    let requests = seen.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .to_ascii_lowercase()
            .contains("user-agent: vaultforge wallet/0.1.0"),
        "retried request carries the app user-agent"
    );
}

#[test]
fn reports_error_when_all_endpoints_fail() {
    let (primary, _, primary_handle) = status_server(500, "{}", 3);
    let (fallback, _, fallback_handle) = status_server(500, "{}", 3);
    let error = runtime()
        .block_on(rpc_post_urls(
            &reqwest::Client::new(),
            &[primary, fallback],
            &post_body(),
        ))
        .unwrap_err();
    assert!(error.contains("attempt 3/3"), "{error}");
    primary_handle.join().unwrap();
    fallback_handle.join().unwrap();
}

#[test]
fn reports_exhausted_attempts_when_connections_refused() {
    let error = runtime()
        .block_on(resilient_get(
            &reqwest::Client::new(),
            "http://127.0.0.1:1/unreachable",
            &[],
        ))
        .unwrap_err();
    assert!(
        error.contains("attempt 3/3"),
        "exhaustion reports all attempts: {error}"
    );
}

/// Serves a scripted sequence of `(status, body, extra_headers)` responses in
/// order, counting hits. Repeats the last scripted response when exhausted.
pub(crate) fn script_server(
    script: Vec<(u16, &'static str, &'static str)>,
) -> (String, Arc<Mutex<usize>>, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(Mutex::new(0usize));
    let counted = hits.clone();
    let handle = std::thread::spawn(move || {
        let total = script.len().max(1);
        for stream in listener.incoming().take(total) {
            let mut stream = match stream {
                Ok(stream) => stream,
                Err(_) => continue,
            };
            let step = (*counted.lock().unwrap()).min(script.len().saturating_sub(1));
            *counted.lock().unwrap() += 1;
            let mut buffer = vec![0u8; 4096];
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let _ = stream.read(&mut buffer);
            let (status, body, extra) = script[step];
            let response = format!(
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (url, hits, handle)
}

#[test]
fn respects_retry_after_on_rate_limited_responses() {
    let (url, hits, handle) = script_server(vec![
        (429, "{}", "Retry-After: 0\r\n"),
        (429, "{}", "Retry-After: 0\r\n"),
        (200, "{\"ok\":true}", ""),
    ]);
    let result = runtime()
        .block_on(super::rpc_post(&reqwest::Client::new(), &url, &post_body()))
        .unwrap();
    assert_eq!(result, serde_json::json!({"ok": true}));
    assert_eq!(*hits.lock().unwrap(), 3);
    handle.join().unwrap();
}

#[test]
fn returns_results_and_preserves_json_rpc_errors() {
    let success = serde_json::json!({ "result": { "value": 1 } });
    assert_eq!(json_rpc_result(&success, "Test").unwrap()["value"], 1);

    let error = serde_json::json!({ "error": { "code": -32000, "message": "rate limited" } });
    assert_eq!(
        json_rpc_result(&error, "Test").unwrap_err(),
        "Test RPC error: {\"code\":-32000,\"message\":\"rate limited\"}"
    );

    assert_eq!(
        json_rpc_result(&serde_json::json!({}), "Test").unwrap_err(),
        "Test RPC response is missing result"
    );
}
