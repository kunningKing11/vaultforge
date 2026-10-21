//! LI.FI exact-input quote boundary; credentials stay in the Rust process.
use crate::registry::NetworkConfig;
use serde_json::Value;

pub(crate) async fn quote(
    client: &reqwest::Client,
    network: &NetworkConfig,
    sender: &str,
    token_in: &str,
    token_out: &str,
    input: &str,
    slippage: u16,
) -> Result<Value, String> {
    let chain = if network.kind == "svm" {
        "1151111081099710".into()
    } else {
        network.chain_id()?.to_string()
    };
    let mut request = client.get("https://li.quest/v1/quote").query(&[
        ("fromChain", chain.as_str()),
        ("toChain", chain.as_str()),
        ("fromAddress", sender),
        ("toAddress", sender),
        ("fromToken", token_in),
        ("toToken", token_out),
        ("fromAmount", input),
        (
            "slippage",
            &format!("{}.{:04}", slippage / 10000, slippage % 10000),
        ),
        ("integrator", "vaultforge"),
    ]);
    if network.kind == "svm" {
        request = request.query(&[
            ("allowExchanges", "jupiter"),
            ("svmMaxSupportedTransactionVersion", "0"),
        ]);
    }
    if let Ok(key) = std::env::var("VAULTFORGE_LIFI_API_KEY") {
        request = request.header("x-lifi-api-key", key);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("LI.FI quote unavailable: {e}"))?;
    if response.status().as_u16() == 429 {
        return Err("LI.FI quote limit reached; wait before refreshing or configure VAULTFORGE_LIFI_API_KEY".into());
    }
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err("LI.FI authentication failed; check VAULTFORGE_LIFI_API_KEY".into());
    }
    if !response.status().is_success() {
        return Err(format!(
            "No LI.FI route available (HTTP {})",
            response.status().as_u16()
        ));
    }
    response
        .json()
        .await
        .map_err(|_| "Invalid LI.FI quote response".into())
}

pub(crate) async fn verify_deployment(
    client: &reqwest::Client,
    config: &NetworkConfig,
) -> Result<(), String> {
    crate::providers::evm::verify_evm_chain(client, config).await?;
    let request = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "eth_getCode",
        "params": [crate::tx::lifi::DIAMOND, "latest"]
    });
    let response =
        crate::providers::http::rpc_post_network(client, &config.id, config.rpc_url()?, &request)
            .await?;
    let code = crate::providers::http::json_rpc_result(&response, "LI.FI contract")?
        .as_str()
        .ok_or("LI.FI contract code missing")?;
    if hex::decode(
        code.strip_prefix("0x")
            .ok_or("Invalid LI.FI contract code")?,
    )
    .map_err(|_| "Invalid LI.FI bytecode")?
    .is_empty()
    {
        return Err("LI.FI execution contract is unavailable on this network".into());
    }
    Ok(())
}
