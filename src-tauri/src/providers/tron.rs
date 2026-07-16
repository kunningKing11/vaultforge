use crate::assets::cached_asset;
use crate::dto::Asset;
use crate::providers::http::rpc_post;

const TRON_RPC_URL: &str = "https://tron-rpc.publicnode.com";
pub(crate) const TRON_NATIVE_FEE_SUN: u64 = 1_000_000;

pub(crate) fn tron_address_to_hex(address: &str) -> Result<String, String> {
    let bytes = bs58::decode(address)
        .with_check(None)
        .into_vec()
        .map_err(|_| "Invalid Tron address".to_string())?;
    if bytes.len() != 21 || bytes[0] != 0x41 {
        return Err("Invalid Tron address prefix".to_string());
    }
    Ok(hex::encode(bytes))
}

pub(crate) async fn fetch_tron_native_balance(address: &str) -> Result<u128, String> {
    let owner_address = tron_address_to_hex(address)?;
    let body = serde_json::json!({
        "address": owner_address,
        "visible": false
    });

    let json = rpc_post(&format!("{TRON_RPC_URL}/wallet/getaccount"), &body).await?;
    Ok(json["balance"].as_u64().unwrap_or(0) as u128)
}

pub(crate) async fn fetch_tron_assets(address: &str, cached_assets: &[Asset]) -> Vec<Asset> {
    let native = match fetch_tron_native_balance(address).await {
        Ok(sun) => Asset {
            symbol: "TRX".to_string(),
            name: "Tron".to_string(),
            balance: sun.to_string(),
            decimals: 6,
            price_usd: 0.0,
            change_24h: 0.0,
            network: "tron".to_string(),
            token_address: None,
        },
        Err(_) => cached_asset(cached_assets, "tron", "TRX").unwrap_or_else(|| Asset {
            symbol: "TRX".to_string(),
            name: "Tron".to_string(),
            balance: "0".to_string(),
            decimals: 6,
            price_usd: 0.0,
            change_24h: 0.0,
            network: "tron".to_string(),
            token_address: None,
        }),
    };

    vec![native]
}

pub(crate) async fn create_tron_transfer(
    from: &str,
    to: &str,
    amount_sun: u64,
) -> Result<serde_json::Value, String> {
    let owner_address = tron_address_to_hex(from)?;
    let to_address = tron_address_to_hex(to)?;
    let body = serde_json::json!({
        "owner_address": owner_address,
        "to_address": to_address,
        "amount": amount_sun,
        "visible": false
    });

    let json = rpc_post(&format!("{TRON_RPC_URL}/wallet/createtransaction"), &body).await?;
    if let Some(error) = json["Error"].as_str().or_else(|| json["error"].as_str()) {
        return Err(error.to_string());
    }
    if json["raw_data_hex"].as_str().is_none() {
        return Err("Tron unsigned transaction missing raw_data_hex".to_string());
    }
    Ok(json)
}

pub(crate) async fn broadcast_tron_transaction(tx: &serde_json::Value) -> Result<String, String> {
    let json = rpc_post(&format!("{TRON_RPC_URL}/wallet/broadcasttransaction"), tx).await?;
    if json["result"].as_bool() == Some(true) {
        return json["txid"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Tron broadcast missing txid".to_string());
    }

    Err(json["message"]
        .as_str()
        .unwrap_or("Unknown Tron broadcast error")
        .to_string())
}

pub(crate) async fn fetch_tron_tx_status(txid: &str) -> Result<Option<String>, String> {
    let json = rpc_post(
        &format!("{TRON_RPC_URL}/wallet/gettransactioninfobyid"),
        &serde_json::json!({ "value": txid }),
    )
    .await?;

    if json.as_object().is_some_and(|obj| obj.is_empty()) {
        return Ok(None);
    }

    match json["receipt"]["result"].as_str() {
        Some("SUCCESS") => Ok(Some("confirmed".to_string())),
        Some(_) => Ok(Some("failed".to_string())),
        None => Ok(None),
    }
}
