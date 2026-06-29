/*
pub(crate) async fn parse_rpc_response(
    url: &str,
    body: &serde_json::Value
) -> Result<serde_json::Value, String> {
    let json = rpc_post("https://solana-rpc.publicnode.com", &body).await?;
    if json.get("error").is_none() {
        Ok(json)
    } else {
        Err(format!("RPC request to {url} failed: {:?}", json.get("error")))
    }
}
*/
pub(crate) async fn rpc_post(
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

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

pub(crate) async fn http_get_json(url: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

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
                    tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64))
                        .await;
                }
                continue;
            }
        };

        if !response.status().is_success() {
            last_err = format!("HTTP returned {} (attempt {attempt}/3)", response.status());
            if attempt < 3 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
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

pub(crate) async fn http_post_text(url: &str, body: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

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
