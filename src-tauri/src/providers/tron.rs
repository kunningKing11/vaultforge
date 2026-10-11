use crate::assets::cached_asset;
use crate::dto::Asset;
use crate::providers::NetworkAssetRefresh;
use crate::providers::http::optional_tx_hash;
use crate::providers::http::{optional_tx_height, rpc_post};
use crate::registry::{NetworkConfig, network_by_id};
use crate::tx::tx_status::{Execution, Finality, TxStatus, legacy_tx_status};

pub(crate) const TRON_NATIVE_FEE_SUN: u64 = 1_000_000;

fn tron_config() -> Result<&'static NetworkConfig, String> {
    network_by_id("tron").ok_or_else(|| "Tron is missing from the network registry".to_string())
}

fn tron_rpc_url() -> Result<&'static str, String> {
    tron_config()?.rpc_url()
}

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

pub(crate) async fn fetch_tron_native_balance(
    client: &reqwest::Client,
    address: &str,
) -> Result<u128, String> {
    let owner_address = tron_address_to_hex(address)?;
    let body = serde_json::json!({
        "address": owner_address,
        "visible": false
    });
    let json = rpc_post(
        client,
        &format!("{}/wallet/getaccount", tron_rpc_url()?),
        &body,
    )
    .await?;
    Ok(json["balance"].as_u64().unwrap_or(0) as u128)
}

pub(crate) async fn fetch_tron_assets(
    client: &reqwest::Client,
    address: &str,
    cached_assets: &[Asset],
) -> NetworkAssetRefresh {
    let config = tron_config().expect("Tron must exist in the generated network registry");
    match fetch_tron_native_balance(client, address).await {
        Ok(sun) => NetworkAssetRefresh {
            assets: vec![Asset {
                symbol: config.native_asset.symbol.clone(),
                unicode_symbol: config.native_asset.unicode_symbol.clone(),
                name: config.native_asset.name.clone(),
                balance: sun.to_string(),
                decimals: config.native_asset.decimals,
                price_usd: 0.0,
                change_24h: 0.0,
                network: "tron".to_string(),
                token_address: None,
            }],
            balance_failed: false,
        },
        Err(_) => NetworkAssetRefresh {
            assets: cached_asset(cached_assets, &config.id, None)
                .into_iter()
                .collect(),
            balance_failed: true,
        },
    }
}

pub(crate) async fn create_tron_transfer(
    client: &reqwest::Client,
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

    let json = rpc_post(
        client,
        &format!("{}/wallet/createtransaction", tron_rpc_url()?),
        &body,
    )
    .await?;
    if let Some(error) = json["Error"].as_str().or_else(|| json["error"].as_str()) {
        return Err(error.to_string());
    }
    if json["raw_data_hex"].as_str().is_none() {
        return Err("Tron unsigned transaction missing raw_data_hex".to_string());
    }
    Ok(json)
}

pub(crate) async fn broadcast_tron_transaction(
    client: &reqwest::Client,
    tx: &serde_json::Value,
) -> Result<String, String> {
    let json = rpc_post(
        client,
        &format!("{}/wallet/broadcasttransaction", tron_rpc_url()?),
        tx,
    )
    .await?;
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

pub(crate) async fn fetch_tron_tx_status(
    client: &reqwest::Client,
    txid: &str,
) -> Result<Option<String>, String> {
    Ok(legacy_tx_status(
        fetch_tron_tx_observation(client, txid).await?,
    ))
}

pub(crate) async fn fetch_tron_tx_observation(
    client: &reqwest::Client,
    txid: &str,
) -> Result<Option<TxStatus>, String> {
    let json = rpc_post(
        client,
        &format!("{}/wallet/gettransactioninfobyid", tron_rpc_url()?),
        &serde_json::json!({ "value": txid }),
    )
    .await?;

    let Some(observation) = parse_tron_tx_observation(&json)? else {
        return Ok(None);
    };
    let solid = rpc_post(
        client,
        &format!("{}/walletsolidity/gettransactioninfobyid", tron_rpc_url()?),
        &serde_json::json!({"value":txid}),
    )
    .await?;
    finalize_tron_observation(observation, &solid, txid)
}

pub(crate) fn finalize_tron_observation(
    observation: TxStatus,
    solid: &serde_json::Value,
    txid: &str,
) -> Result<Option<TxStatus>, String> {
    let Some(mut final_observation) = parse_tron_tx_observation(solid)? else {
        return Ok(Some(observation));
    };
    let id = solid["id"]
        .as_str()
        .ok_or_else(|| "Missing Tron solidified transaction ID".to_string())?;
    if !id.eq_ignore_ascii_case(txid) {
        return Err("Tron solidified receipt is for another transaction".to_string());
    }
    if final_observation.block_height.is_none() {
        return Err("Missing Tron solidified block height".to_string());
    }
    // The solidified receipt supersedes full-node evidence that may be from an older fork.
    final_observation.finality = Finality::Finalized;
    Ok(Some(final_observation))
}

pub(crate) fn parse_tron_tx_observation(
    json: &serde_json::Value,
) -> Result<Option<TxStatus>, String> {
    let object = json
        .as_object()
        .ok_or_else(|| "Invalid Tron transaction info".to_string())?;
    if object.is_empty() {
        return Ok(None);
    }
    if let Some(error) = object.get("Error") {
        return Err(format!("Tron transaction lookup error: {error}"));
    }
    let transaction_result = match json.get("result") {
        Some(serde_json::Value::String(result)) if result == "SUCESS" => Some(Execution::Succeeded),
        Some(serde_json::Value::String(result)) if result == "FAILED" => Some(Execution::Failed),
        None => None,
        _ => return Err("Invalid Tron transaction result".to_string()),
    };
    let receipt_result = match json["receipt"].get("result") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or_else(|| "Invalid Tron receipt result".to_string())?,
        ),
    };
    let execution = if transaction_result == Some(Execution::Failed) {
        Execution::Failed
    } else {
        match receipt_result {
            Some("SUCCESS") => Execution::Succeeded,
            Some(
                "REVERT"
                | "BAD_JUMP_DESTINATION"
                | "OUT_OF_MEMORY"
                | "PRECOMPILED_CONTRACT"
                | "STACK_TOO_SMALL"
                | "STACK_TOO_LARGE"
                | "ILLEGAL_OPERATION"
                | "STACK_OVERFLOW"
                | "OUT_OF_ENERGY"
                | "OUT_OF_TIME"
                | "JVM_STACK_OVER_FLOW"
                | "UNKNOWN"
                | "TRANSFER_FAILED"
                | "INVALID_CODE",
            ) => Execution::Failed,
            Some("DEFAULT") | None => match transaction_result {
                Some(result) => result,
                None => {
                    // TransactionInfo's success code is protobuf's omitted zero default.
                    // Only accept that default for a complete, identified included receipt.
                    if optional_tx_hash(json, "id", "")?.is_some()
                        && optional_tx_height(json, "blockNumber")?.is_some()
                        && optional_tx_height(json, "blockTimeStamp")?.is_some()
                    {
                        Execution::Succeeded
                    } else {
                        return Ok(None);
                    }
                }
            },
            _ => return Err("Invalid Tron receipt result".to_string()),
        }
    };
    let mut observation = TxStatus::new(execution, Finality::Included);
    observation.block_height = optional_tx_height(json, "blockNumber")?;
    Ok(Some(observation))
}
#[cfg(test)]
#[path = "../tests/providers/tron.rs"]
mod tests;
