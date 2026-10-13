use std::time::Duration;

pub(crate) fn optional_tx_height(
    value: &serde_json::Value,
    field: &str,
) -> Result<Option<u64>, String> {
    match value.get(field) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(number) => number
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("Invalid transaction {field}")),
    }
}

pub(crate) fn optional_tx_hash(
    value: &serde_json::Value,
    field: &str,
    prefix: &str,
) -> Result<Option<String>, String> {
    match value.get(field) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => {
            let hash = value
                .as_str()
                .ok_or_else(|| format!("Invalid transaction {field}"))?;
            let hex = hash
                .strip_prefix(prefix)
                .ok_or_else(|| format!("Invalid transaction {field}"))?;
            if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(format!("Invalid transaction {field}"));
            }
            Ok(Some(hash.to_string()))
        }
    }
}

pub(crate) struct ProviderClients {
    http: reqwest::Client,
}

impl ProviderClients {
    pub(crate) fn new() -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|error| format!("Failed to create HTTP client: {error}"))?;

        Ok(Self { http })
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.http
    }
}

/// Posts JSON-RPC and retries transient failures up to three attempts.
/// Honours `Retry-After` on rate-limited responses so repeated polls back off
/// instead of hammering a throttled endpoint; errors name the failing host.
pub(crate) async fn rpc_post(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut last_err = String::new();
    for attempt in 1..=3 {
        let response = match send_with_retry(
            || with_user_agent(client.post(url).json(body)),
            url,
            "RPC request",
        )
        .await
        {
            Ok(response) => response,
            Err(error) => return Err(error),
        };

        if !response.status().is_success() {
            let status = response.status();
            let retry_after_secs = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());
            last_err = format!(
                "RPC {} returned HTTP {} (attempt {attempt}/3)",
                rpc_host(url),
                status
            );
            if attempt < 3 {
                tokio::time::sleep(http_retry_delay(url, attempt, retry_after_secs)).await;
            }
            continue;
        }

        return response
            .json()
            .await
            .map_err(|e| format!("RPC response parse failed: {e}"));
    }
    Err(last_err)
}

fn rpc_host(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_owned))
        .unwrap_or_else(|| "endpoint".to_string())
}

/// Spare public JSON-RPC endpoints per EVM network, tried in order when the
/// configured endpoint fails. Same network only: observations re-validate
/// finalized heads and canonical blocks, and nonces resolve with max.
/// Kept in code (not the registry) so no schema migration is needed. Every
/// entry below was verified to serve its chain over plain HTTPS; networks
/// without a verified spare resolve to the configured endpoint only.
pub(crate) fn fallback_rpc_urls(network_id: &str) -> &'static [&'static str] {
    match network_id {
        "ethereum" => &["https://cloudflare-eth.com"],
        "avalanche_c" => &["https://api.avax.network/ext/bc/C/rpc"],
        "optimism" => &["https://mainnet.optimism.io"],
        "arbitrum_one" => &["https://arb1.arbitrum.io/rpc"],
        "base" => &["https://base-rpc.publicnode.com"],
        _ => &[],
    }
}

/// Posts JSON-RPC to each URL in order, returning the first success.
pub(crate) async fn rpc_post_urls(
    client: &reqwest::Client,
    urls: &[String],
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut last_err = "No RPC endpoint available".to_string();
    for url in urls {
        match rpc_post(client, url, body).await {
            Ok(json) => return Ok(json),
            Err(error) => last_err = error,
        }
    }
    Err(last_err)
}

/// Posts JSON-RPC to the configured endpoint, then same-network public
/// fallbacks. The happy path is unchanged: fallbacks run only after the
/// primary exhausts its retries.
pub(crate) async fn rpc_post_network(
    client: &reqwest::Client,
    network_id: &str,
    primary_url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut urls = vec![primary_url.to_string()];
    urls.extend(
        fallback_rpc_urls(network_id)
            .iter()
            .map(|url| url.to_string()),
    );
    rpc_post_urls(client, &urls, body).await
}

pub(crate) fn json_rpc_result<'a>(
    response: &'a serde_json::Value,
    operation: &str,
) -> Result<&'a serde_json::Value, String> {
    if let Some(error) = response.get("error").filter(|error| !error.is_null()) {
        return Err(format!("{operation} RPC error: {error}"));
    }
    response
        .get("result")
        .ok_or_else(|| format!("{operation} RPC response is missing result"))
}

pub(crate) async fn http_get_json(
    client: &reqwest::Client,
    url: &str,
) -> Result<serde_json::Value, String> {
    http_get_json_with_client(client, url).await
}

/// Fetches JSON with `Retry-After` handling and URL-jittered retries.
pub(crate) async fn http_get_json_with_client(
    client: &reqwest::Client,
    url: &str,
) -> Result<serde_json::Value, String> {
    let mut last_err = String::new();
    for attempt in 1..=3 {
        let response = match send_with_retry(
            || with_user_agent(client.get(url).header("accept", "application/json")),
            url,
            "HTTP request",
        )
        .await
        {
            Ok(response) => response,
            Err(error) => return Err(error),
        };

        if !response.status().is_success() {
            let retry_after_secs = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());
            last_err = format!("HTTP returned {} (attempt {attempt}/3)", response.status());
            if attempt < 3 {
                tokio::time::sleep(http_retry_delay(url, attempt, retry_after_secs)).await;
            }
            continue;
        }

        return response
            .json()
            .await
            .map_err(|e| format!("HTTP response parse failed: {e}"));
    }
    Err(last_err)
}

fn http_retry_delay(url: &str, attempt: u64, retry_after_secs: Option<u64>) -> std::time::Duration {
    let jitter_ms = url
        .bytes()
        .fold(0u64, |total, byte| total.wrapping_add(u64::from(byte)))
        % 250;
    let base_ms = retry_after_secs
        .map(|seconds| seconds.min(30).saturating_mul(1_000))
        .unwrap_or_else(|| 500u64.saturating_mul(attempt));
    std::time::Duration::from_millis(base_ms.saturating_add(jitter_ms))
}

pub(crate) async fn http_post_text(
    client: &reqwest::Client,
    url: &str,
    body: &str,
) -> Result<String, String> {
    let mut last_err = String::new();
    for attempt in 1..=3 {
        let response = match send_with_retry(
            || {
                with_user_agent(
                    client
                        .post(url)
                        .body(body.to_string())
                        .header("content-type", "text/plain"),
                )
            },
            url,
            "HTTP POST",
        )
        .await
        {
            Ok(response) => response,
            Err(error) => return Err(error),
        };

        if !response.status().is_success() {
            let status = response.status();
            let retry_after_secs = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());
            let text = response.text().await.unwrap_or_default();
            last_err = format!("HTTP POST returned {status} (attempt {attempt}/3): {text}");
            if attempt < 3 {
                tokio::time::sleep(http_retry_delay(url, attempt, retry_after_secs)).await;
            }
            continue;
        }

        return response
            .text()
            .await
            .map_err(|e| format!("HTTP response parse failed: {e}"));
    }
    Err(last_err)
}

/// GETs a URL for callers that must interpret statuses themselves (Circle
/// fee/attestation reads). Transport errors share the single retry driver;
/// callers still fail closed on missing or invalid payloads.
pub(crate) async fn resilient_get(
    client: &reqwest::Client,
    url: &str,
    query: &[(&str, &str)],
) -> Result<reqwest::Response, String> {
    let context = format!("GET {url}");
    send_with_retry(
        || with_user_agent(client.get(url).query(query)),
        url,
        &context,
    )
    .await
}

const APP_USER_AGENT: &str = "VaultForge Wallet/0.1.0";

fn with_user_agent(builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    builder.header("user-agent", APP_USER_AGENT)
}

/// Sends one request, retrying transport errors up to three attempts with the
/// shared jittered backoff. HTTP statuses pass through untouched.
async fn send_with_retry(
    build: impl Fn() -> reqwest::RequestBuilder,
    url: &str,
    context: &str,
) -> Result<reqwest::Response, String> {
    let mut last_err = String::new();
    for attempt in 1..=3 {
        match build().send().await {
            Ok(response) => return Ok(response),
            Err(error) => {
                last_err = format!("{context} failed (attempt {attempt}/3): {error}");
                if attempt < 3 {
                    tokio::time::sleep(http_retry_delay(url, attempt, None)).await;
                }
            }
        }
    }
    Err(last_err)
}

#[cfg(test)]
#[path = "../tests/providers/http.rs"]
pub(crate) mod tests;
