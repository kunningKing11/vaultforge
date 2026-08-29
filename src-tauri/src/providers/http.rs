use std::time::Duration;

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

pub(crate) async fn rpc_post(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut last_err = String::new();
    for attempt in 1..=3 {
        let response = match client
            .post(url)
            .json(body)
            .header("user-agent", "VaultForge Wallet/0.1.0")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("RPC request failed (attempt {attempt}/3): {e}");
                if attempt < 3 {
                    tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64))
                        .await;
                }
                continue;
            }
        };

        if !response.status().is_success() {
            last_err = format!(
                "RPC returned HTTP {} (attempt {attempt}/3)",
                response.status()
            );
            if attempt < 3 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
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

pub(crate) async fn http_get_json(
    client: &reqwest::Client,
    url: &str,
) -> Result<serde_json::Value, String> {
    http_get_json_with_client(client, url).await
}
pub(crate) async fn http_get_json_with_client(
    client: &reqwest::Client,
    url: &str,
) -> Result<serde_json::Value, String> {
    let mut last_err = String::new();
    for attempt in 1..=3 {
        let response = match client
            .get(url)
            .header("accept", "application/json")
            .header("user-agent", "VaultForge Wallet/0.1.0")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("HTTP request failed (attempt {attempt}/3): {e}");
                if attempt < 3 {
                    tokio::time::sleep(http_retry_delay(url, attempt, None)).await;
                }
                continue;
            }
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
        let response = match client
            .post(url)
            .body(body.to_string())
            .header("content-type", "text/plain")
            .header("user-agent", "VaultForge Wallet/0.1.0")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("HTTP POST failed (attempt {attempt}/3): {e}");
                if attempt < 3 {
                    tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64))
                        .await;
                }
                continue;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            last_err = format!("HTTP POST returned {status} (attempt {attempt}/3): {text}");
            if attempt < 3 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
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
