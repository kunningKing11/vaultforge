use serde_json::{Value, json};

use crate::assets::cached_asset;
use crate::dto::Asset;
use crate::providers::NetworkAssetRefresh;
use crate::providers::http::{json_rpc_result, rpc_post};
use crate::registry::{NetworkConfig, network_by_id};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct XrplAccountInfo {
    pub(crate) balance_drops: u64,
    pub(crate) owner_count: u32,
    pub(crate) sequence: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct XrplNetworkInfo {
    pub(crate) base_reserve_drops: u64,
    pub(crate) owner_reserve_drops: u64,
    pub(crate) open_ledger_fee_drops: u64,
    pub(crate) validated_ledger_index: u32,
}

fn xrpl_config() -> Result<&'static NetworkConfig, String> {
    network_by_id("xrpl")
        .ok_or_else(|| "XRP Ledger is missing from the network registry".to_string())
}

fn xrpl_rpc_url() -> Result<&'static str, String> {
    xrpl_config()?.rpc_url()
}

async fn xrpl_rpc(client: &reqwest::Client, method: &str, params: Value) -> Result<Value, String> {
    let json = rpc_post(
        client,
        xrpl_rpc_url()?,
        &json!({
            "method": method,
            "params": [params],
        }),
    )
    .await?;
    json_rpc_result(&json, &format!("XRP Ledger {method}"))?;
    Ok(json)
}

fn rpc_error(result: &Value) -> Option<&str> {
    result
        .get("error")
        .and_then(Value::as_str)
        .or_else(|| result.get("error_message").and_then(Value::as_str))
}

fn parse_u64(value: &Value, field: &str) -> Result<u64, String> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .ok_or_else(|| format!("XRP Ledger response has invalid {field}"))
}

fn parse_u32(value: &Value, field: &str) -> Result<u32, String> {
    parse_u64(value, field)?
        .try_into()
        .map_err(|_| format!("XRP Ledger response has out-of-range {field}"))
}

fn xrp_to_drops(value: &Value, field: &str) -> Result<u64, String> {
    let value = value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_number().map(ToString::to_string))
        .ok_or_else(|| format!("XRP Ledger response has invalid {field}"))?;
    let (whole, fraction) = value.split_once('.').unwrap_or((&value, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() > 6
    {
        return Err(format!("XRP Ledger response has invalid {field}"));
    }
    let whole_drops = whole
        .parse::<u64>()
        .map_err(|_| format!("XRP Ledger response has invalid {field}"))?
        .checked_mul(1_000_000)
        .ok_or_else(|| format!("XRP Ledger response has out-of-range {field}"))?;
    let fraction_drops = if fraction.is_empty() {
        0
    } else {
        let mut fraction = fraction.to_string();
        fraction.push_str(&"0".repeat(6 - fraction.len()));
        fraction
            .parse::<u64>()
            .map_err(|_| format!("XRP Ledger response has invalid {field}"))?
    };
    whole_drops
        .checked_add(fraction_drops)
        .ok_or_else(|| format!("XRP Ledger response has out-of-range {field}"))
}

pub(crate) fn parse_xrpl_account_info(json: &Value) -> Result<Option<XrplAccountInfo>, String> {
    let result = json_rpc_result(json, "XRP Ledger account_info")?;
    if matches!(rpc_error(result), Some("actNotFound")) {
        return Ok(None);
    }
    if let Some(error) = rpc_error(result) {
        return Err(format!("XRP Ledger account_info error: {error}"));
    }
    let account = result
        .get("account_data")
        .ok_or_else(|| "XRP Ledger account_info response is missing account_data".to_string())?;
    Ok(Some(XrplAccountInfo {
        balance_drops: parse_u64(&account["Balance"], "account balance")?,
        owner_count: parse_u32(&account["OwnerCount"], "owner count")?,
        sequence: parse_u32(&account["Sequence"], "sequence")?,
    }))
}

pub(crate) fn parse_xrpl_network_info(
    fee_json: &Value,
    server_info_json: &Value,
) -> Result<XrplNetworkInfo, String> {
    let fee_result = json_rpc_result(fee_json, "XRP Ledger fee")?;
    if let Some(error) = rpc_error(fee_result) {
        return Err(format!("XRP Ledger fee error: {error}"));
    }
    let server_result = json_rpc_result(server_info_json, "XRP Ledger server_info")?;
    if let Some(error) = rpc_error(server_result) {
        return Err(format!("XRP Ledger server_info error: {error}"));
    }
    let ledger = &server_result["info"]["validated_ledger"];
    Ok(XrplNetworkInfo {
        base_reserve_drops: xrp_to_drops(&ledger["reserve_base_xrp"], "base reserve")?,
        owner_reserve_drops: xrp_to_drops(&ledger["reserve_inc_xrp"], "owner reserve")?,
        open_ledger_fee_drops: parse_u64(
            &fee_result["drops"]["open_ledger_fee"],
            "open ledger fee",
        )?,
        validated_ledger_index: parse_u32(&ledger["seq"], "validated ledger index")?,
    })
}

pub(crate) async fn fetch_xrpl_account_info(
    client: &reqwest::Client,
    address: &str,
    ledger_index: &str,
) -> Result<Option<XrplAccountInfo>, String> {
    let json = xrpl_rpc(
        client,
        "account_info",
        json!({ "account": address, "ledger_index": ledger_index, "queue": ledger_index == "current", "strict": true }),
    )
    .await?;
    parse_xrpl_account_info(&json)
}

pub(crate) async fn fetch_xrpl_network_info(
    client: &reqwest::Client,
) -> Result<XrplNetworkInfo, String> {
    let fee = xrpl_rpc(client, "fee", json!({})).await?;
    let server_info = xrpl_rpc(client, "server_info", json!({})).await?;
    parse_xrpl_network_info(&fee, &server_info)
}

pub(crate) async fn fetch_xrpl_assets(
    client: &reqwest::Client,
    address: &str,
    cached_assets: &[Asset],
) -> NetworkAssetRefresh {
    let config = xrpl_config().expect("XRP Ledger must exist in the generated network registry");
    match fetch_xrpl_account_info(client, address, "validated").await {
        Ok(account) => NetworkAssetRefresh {
            assets: vec![Asset {
                symbol: config.native_asset.symbol.clone(),
                unicode_symbol: config.native_asset.unicode_symbol.clone(),
                name: config.native_asset.name.clone(),
                balance: account
                    .map(|account| account.balance_drops.to_string())
                    .unwrap_or_else(|| "0".to_string()),
                decimals: config.native_asset.decimals,
                price_usd: 0.0,
                change_24h: 0.0,
                network: config.id.clone(),
                token_address: None,
            }],
            balance_failed: false,
        },
        Err(_) => NetworkAssetRefresh {
            assets: cached_asset(cached_assets, &config.id, &config.native_asset.symbol)
                .into_iter()
                .collect(),
            balance_failed: true,
        },
    }
}

pub(crate) async fn broadcast_xrpl_transaction(
    client: &reqwest::Client,
    raw_tx_hex: &str,
    expected_hash: &str,
) -> Result<String, String> {
    let json = xrpl_rpc(
        client,
        "submit",
        json!({ "tx_blob": raw_tx_hex, "fail_hard": true }),
    )
    .await?;
    let result = json_rpc_result(&json, "XRP Ledger submit")?;
    let engine_result = result["engine_result"].as_str().unwrap_or_default();
    if engine_result != "tesSUCCESS" && engine_result != "terQUEUED" {
        return Err(result["engine_result_message"]
            .as_str()
            .unwrap_or(engine_result)
            .to_string());
    }
    let hash = result["tx_json"]["hash"]
        .as_str()
        .ok_or_else(|| "XRP Ledger broadcast response is missing transaction hash".to_string())?;
    if hash != expected_hash {
        return Err(
            "XRP Ledger broadcast hash does not match the locally signed transaction".to_string(),
        );
    }
    Ok(hash.to_string())
}

pub(crate) async fn fetch_xrpl_tx_status(
    client: &reqwest::Client,
    tx_hash: &str,
) -> Result<Option<String>, String> {
    let json = xrpl_rpc(
        client,
        "tx",
        json!({ "transaction": tx_hash, "binary": false }),
    )
    .await?;
    let result = json_rpc_result(&json, "XRP Ledger transaction lookup")?;
    if matches!(rpc_error(result), Some("txnNotFound")) {
        return Ok(None);
    }
    if let Some(error) = rpc_error(result) {
        return Err(format!("XRP Ledger transaction lookup error: {error}"));
    }
    if result["validated"].as_bool() != Some(true) {
        return Ok(None);
    }
    match result["meta"]["TransactionResult"].as_str() {
        Some("tesSUCCESS") => Ok(Some("confirmed".to_string())),
        Some(_) => Ok(Some("failed".to_string())),
        None => Err("XRP Ledger validated transaction is missing its result".to_string()),
    }
}

#[cfg(test)]
#[path = "../tests/providers/xrpl.rs"]
mod tests;
