//! Jupiter Swap V2 HTTP requests and response parsing.

use reqwest::Client;
use serde_json::Value;

pub(crate) const ORDER_URL: &str = "https://api.jup.ag/swap/v2/order";
pub(crate) const EXECUTE_URL: &str = "https://api.jup.ag/swap/v2/execute";

/// Quote inputs derived from the unlocked wallet and selected assets.
pub(crate) struct OrderRequest {
    pub(crate) taker: String,
    pub(crate) input_mint: String,
    pub(crate) output_mint: String,
    /// Integer amount in input-token base units.
    pub(crate) amount_base_units: String,
    /// Slippage cap in basis points; 50 means 0.5%.
    pub(crate) slippage_bps: u16,
}

/// Parsed response; `tx::jupiter::validate_order` authorizes the transaction.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub(crate) struct JupiterOrder {
    pub(crate) request_id: String,

    pub(crate) router: String,
    pub(crate) transaction_base64: String,
    pub(crate) quoted_output_base_units: String,
    /// Minimum output to verify against the transaction instructions.
    pub(crate) minimum_output_base_units: String,
    /// Provider metadata retained for fee and expiry checks.
    pub(crate) raw_response: Value,
}

/// Execution payload containing the saved request ID and public signed bytes.
pub(crate) struct ExecuteRequest<'a> {
    pub(crate) request_id: &'a str,
    pub(crate) signed_transaction_base64: &'a str,
}

/// Service report; completion requires independent chain evidence.
pub(crate) struct JupiterExecution {
    pub(crate) raw_response: Value,
}

/// Map native SOL sentinels to the WSOL mint used by Jupiter.
pub(crate) fn normalize_mint(mint: &str) -> &str {
    if mint == crate::tx::swap_solana::NATIVE || mint.eq_ignore_ascii_case(crate::tx::relay::NATIVE)
    {
        crate::tx::swap_solana::WSOL
    } else {
        mint
    }
}

fn order_string<'a>(raw_response: &'a Value, field: &str) -> Result<&'a str, String> {
    raw_response
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Jupiter order response is missing {field}"))
}

fn order_amount(raw_response: &Value, field: &str) -> Result<u64, String> {
    let amount = raw_response
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Jupiter order response is missing {field}"))?;
    if amount.is_empty() || !amount.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("Jupiter order response has an invalid {field}"));
    }
    amount
        .parse::<u64>()
        .map_err(|_| format!("Jupiter order response has an invalid {field}"))
}

/// Bind response metadata to the request before transaction validation.
fn parse_order(
    raw_response: Value,
    request: &OrderRequest,
    requested_amount: u64,
) -> Result<JupiterOrder, String> {
    let input_mint = normalize_mint(&request.input_mint);
    let output_mint = normalize_mint(&request.output_mint);
    let returned_input_mint = order_string(&raw_response, "inputMint")?;
    let returned_output_mint = order_string(&raw_response, "outputMint")?;
    if returned_input_mint != input_mint {
        return Err("Jupiter returned a different input mint".into());
    }
    if returned_output_mint != output_mint {
        return Err("Jupiter returned a different output mint".into());
    }

    let input_amount = order_amount(&raw_response, "inAmount")?;
    if input_amount != requested_amount {
        return Err("Jupiter returned a different input amount".into());
    }
    let returned_slippage_bps = raw_response
        .get("slippageBps")
        .and_then(Value::as_u64)
        .ok_or("Jupiter order response is missing slippageBps")?;
    if returned_slippage_bps != u64::from(request.slippage_bps) {
        return Err("Jupiter returned a different slippage limit".into());
    }
    if let Some(returned_taker) = raw_response.get("taker").and_then(Value::as_str) {
        if returned_taker != request.taker {
            return Err("Jupiter returned a different taker".into());
        }
    }

    let quoted_output = order_amount(&raw_response, "outAmount")?;
    let minimum_output = order_amount(&raw_response, "otherAmountThreshold")?;
    if quoted_output == 0 || minimum_output == 0 {
        return Err("Jupiter returned a non-positive output amount".into());
    }
    if minimum_output > quoted_output {
        return Err("Jupiter minimum output exceeds the quoted output".into());
    }
    if request.slippage_bps > 10_000 {
        return Err("Jupiter slippage cannot exceed 10000 basis points".into());
    }
    let slippage_floor =
        u128::from(quoted_output) * u128::from(10_000 - u64::from(request.slippage_bps)) / 10_000;
    if u128::from(minimum_output) < slippage_floor {
        return Err("Jupiter minimum output exceeds the requested slippage".into());
    }

    let request_id = order_string(&raw_response, "requestId")?.to_owned();
    let router = order_string(&raw_response, "router")?.to_owned();
    let transaction_base64 = raw_response
        .get("transaction")
        .and_then(Value::as_str)
        .filter(|transaction| !transaction.is_empty())
        .ok_or_else(|| {
            let error_code = raw_response.get("errorCode").and_then(Value::as_u64);
            match error_code {
                Some(code) => {
                    format!("Jupiter could not build a transaction ({router} error {code})")
                }
                None => "Jupiter returned no executable transaction".to_string(),
            }
        })?
        .to_owned();
    if raw_response
        .get("transactionVersion")
        .and_then(Value::as_u64)
        != Some(0)
    {
        return Err("Jupiter returned an unsupported transaction version".into());
    }

    Ok(JupiterOrder {
        request_id,
        router,
        transaction_base64,
        quoted_output_base_units: quoted_output.to_string(),
        minimum_output_base_units: minimum_output.to_string(),
        raw_response,
    })
}

/// Fetch an order without signing or submitting it.
pub(crate) async fn get_order(
    client: &Client,
    request: &OrderRequest,
) -> Result<JupiterOrder, String> {
    if request.slippage_bps > 10_000 {
        return Err("Jupiter slippage cannot exceed 10000 basis points".into());
    }
    let requested_amount = request
        .amount_base_units
        .parse::<u64>()
        .map_err(|_| "Jupiter order amount is not a valid base-unit integer".to_string())?;
    if requested_amount == 0 {
        return Err("Jupiter order amount must be greater than zero".into());
    }
    let input_mint = normalize_mint(&request.input_mint);
    let output_mint = normalize_mint(&request.output_mint);
    let slippage_bps_text = request.slippage_bps.to_string();
    let max_transaction_version_text = "0".to_string();
    let query_params = [
        ("inputMint", input_mint),
        ("outputMint", output_mint),
        ("amount", request.amount_base_units.as_str()),
        ("taker", request.taker.as_str()),
        ("excludeRouters", "jupiterz,dflow,okx"),
        ("slippageBps", slippage_bps_text.as_str()),
        (
            "maxSupportedTransactionVersion",
            max_transaction_version_text.as_str(),
        ),
    ];
    let order_request = client
        .get(ORDER_URL)
        .timeout(std::time::Duration::from_secs(30))
        .query(&query_params);
    let order_request = if let Ok(api_key) = std::env::var("VAULTFORGE_JUPITER_API_KEY") {
        order_request.header("x-api-key", api_key)
    } else {
        order_request
    };
    let response = order_request.send().await.map_err(|error| {
        if error.is_timeout() {
            "Jupiter order request timed out; refresh the quote.".to_string()
        } else if error.is_connect() {
            "Could not reach Jupiter; check your connection and refresh.".to_string()
        } else {
            format!("Jupiter order request failed: {error}")
        }
    })?;

    let status = response.status();
    if !status.is_success() {
        return match status.as_u16() {
            400 => {
                let detail = response
                    .json::<Value>()
                    .await
                    .ok()
                    .and_then(|body| body.get("error").and_then(Value::as_str).map(str::to_owned))
                    .unwrap_or_else(|| "invalid order request".to_string());
                Err(format!("Jupiter rejected the order request: {detail}"))
            }
            401 | 403 => {
                Err("Jupiter authentication failed; check VAULTFORGE_JUPITER_API_KEY".to_string())
            }
            429 => Err("Jupiter rate limit reached; wait before refreshing the quote".to_string()),
            500..=599 => {
                Err("Jupiter is temporarily unavailable; refresh the quote later".to_string())
            }
            other => Err(format!("Jupiter order request failed (HTTP {other})")),
        };
    }

    let raw_response = response
        .json::<Value>()
        .await
        .map_err(|_| "Jupiter returned an invalid order response".to_string())?;
    parse_order(raw_response, request, requested_amount)
}

/// Execute the saved signed order once. Transport errors leave submission uncertain.
pub(crate) async fn execute_order(
    client: &Client,
    request: &ExecuteRequest<'_>,
) -> Result<JupiterExecution, String> {
    let mut http = client
        .post(EXECUTE_URL)
        .timeout(std::time::Duration::from_secs(30))
        .json(&serde_json::json!({
            "requestId": request.request_id,
            "signedTransaction": request.signed_transaction_base64,
        }));
    if let Ok(api_key) = std::env::var("VAULTFORGE_JUPITER_API_KEY") {
        http = http.header("x-api-key", api_key);
    }
    let response = http
        .send()
        .await
        .map_err(|_| "Jupiter execution response unavailable; reconcile the saved signature")?;
    if !response.status().is_success() {
        return Err(format!(
            "Jupiter execution HTTP {}; reconcile the saved signature",
            response.status().as_u16()
        ));
    }
    let raw_response: Value = response
        .json()
        .await
        .map_err(|_| "Invalid Jupiter execution response; reconcile the saved signature")?;
    if !matches!(raw_response["status"].as_str(), Some("Success" | "Failed"))
        || raw_response["code"].as_i64().is_none()
    {
        return Err("Invalid Jupiter execution report; reconcile the saved signature".into());
    }
    Ok(JupiterExecution { raw_response })
}

#[cfg(test)]
#[path = "../tests/providers/jupiter.rs"]
mod tests;
