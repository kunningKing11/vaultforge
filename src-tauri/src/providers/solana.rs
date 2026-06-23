use crate::providers::http::rpc_post;

pub(crate) async fn fetch_solana_balance(address: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getBalance",
        "params": [address],
        "id": 1,
    });
    let json = rpc_post("https://api.mainnet-beta.solana.com", &body).await?;
    parse_solana_balance(&json).map(|lamports| lamports.to_string())
}

pub(crate) fn parse_solana_balance(json: &serde_json::Value) -> Result<u128, String> {
    json["result"]["value"]
        .as_u64()
        .map(|value| value as u128)
        .ok_or_else(|| "Solana balance RPC missing result.value".to_string())
}
