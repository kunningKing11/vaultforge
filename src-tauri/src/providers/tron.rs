use crate::assets::cached_asset;
use crate::dto::Asset;
use crate::providers::NetworkAssetRefresh;
use crate::providers::http::{optional_tx_hash, optional_tx_height, rpc_post};
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
        Ok(sun) => {
            let mut assets = vec![Asset {
                solana_token: None,
                symbol: config.native_asset.symbol.clone(),
                unicode_symbol: config.native_asset.unicode_symbol.clone(),
                name: config.native_asset.name.clone(),
                balance: sun.to_string(),
                decimals: config.native_asset.decimals,
                price_usd: 0.0,
                change_24h: 0.0,
                network: "tron".to_string(),
                token_address: None,
            }];
            let mut balance_failed = false;
            for token in &config.tokens {
                let Some(token_address) = token.token_address.as_deref() else {
                    continue;
                };
                match fetch_tron_token_balance(client, address, token_address).await {
                    Ok(balance) => assets.push(Asset {
                        solana_token: None,
                        symbol: token.symbol.clone(),
                        unicode_symbol: token.unicode_symbol.clone(),
                        name: token.name.clone(),
                        balance: balance.to_string(),
                        decimals: token.decimals,
                        price_usd: 0.0,
                        change_24h: 0.0,
                        network: config.id.clone(),
                        token_address: Some(token_address.to_string()),
                    }),
                    Err(_) => {
                        balance_failed = true;
                        if let Some(cached) =
                            cached_asset(cached_assets, &config.id, Some(token_address))
                        {
                            assets.push(cached);
                        }
                    }
                }
            }
            NetworkAssetRefresh {
                assets,
                balance_failed,
            }
        }
        Err(_) => NetworkAssetRefresh {
            assets: cached_assets
                .iter()
                .filter(|asset| asset.network == config.id)
                .cloned()
                .collect(),
            balance_failed: true,
        },
    }
}

/// Reads a configured TRC-20 balance using TronGrid's constant-contract endpoint.
pub(crate) async fn fetch_tron_token_balance(
    client: &reqwest::Client,
    owner: &str,
    contract: &str,
) -> Result<u128, String> {
    configured_tron_token(contract)?;
    let owner =
        hex::decode(tron_address_to_hex(owner)?).map_err(|_| "Invalid Tron owner address")?;
    let parameter = format!("{}{}", "0".repeat(24), hex::encode(&owner[1..]));
    let response = rpc_post(
        client,
        &format!("{}/wallet/triggerconstantcontract", tron_rpc_url()?),
        &serde_json::json!({
            "owner_address": hex::encode(owner),
            "contract_address": tron_address_to_hex(contract)?,
            "function_selector": "balanceOf(address)",
            "parameter": parameter,
            "visible": false
        }),
    )
    .await?;
    if response["result"]["result"].as_bool() != Some(true) {
        return Err("Tron TRC-20 balance query failed".into());
    }
    let value = response["constant_result"][0]
        .as_str()
        .ok_or("Tron TRC-20 balance response is missing")?;
    let bytes = hex::decode(value).map_err(|_| "Invalid Tron TRC-20 balance")?;
    if bytes.len() != 32 || bytes[..16].iter().any(|byte| *byte != 0) {
        return Err("Tron TRC-20 balance exceeds supported amount range".into());
    }
    Ok(u128::from_be_bytes(
        bytes[16..]
            .try_into()
            .map_err(|_| "Invalid TRC-20 balance")?,
    ))
}

fn configured_tron_token(contract: &str) -> Result<&'static crate::registry::AssetConfig, String> {
    let config = tron_config()?;
    config
        .tokens
        .iter()
        .find(|token| token.token_address.as_deref() == Some(contract))
        .ok_or_else(|| "TRC-20 contract is not in the wallet token registry".into())
}

/// Builds a fee-capped TRC-20 `transfer(address,uint256)` transaction.
pub(crate) async fn create_tron_token_transfer(
    client: &reqwest::Client,
    from: &str,
    to: &str,
    contract: &str,
    amount: u128,
) -> Result<(serde_json::Value, u64), String> {
    configured_tron_token(contract)?;
    let from_hex = tron_address_to_hex(from)?;
    let to_hex = tron_address_to_hex(to)?;
    let contract_hex = tron_address_to_hex(contract)?;
    let parameter = format!(
        "{}{}{}",
        "0".repeat(24),
        &to_hex[2..],
        format!("{amount:064x}")
    );
    let estimate = rpc_post(
        client,
        &format!("{}/wallet/estimateenergy", tron_rpc_url()?),
        &serde_json::json!({
            "owner_address": from_hex,
            "contract_address": contract_hex,
            "function_selector": "transfer(address,uint256)",
            "parameter": parameter,
            "visible": false
        }),
    )
    .await?;
    let energy = estimate["energy_required"]
        .as_u64()
        .or_else(|| estimate["energy_used"].as_u64())
        .ok_or("Tron energy estimate unavailable")?;
    let parameters = rpc_post(
        client,
        &format!("{}/wallet/getchainparameters", tron_rpc_url()?),
        &serde_json::json!({}),
    )
    .await?;
    let energy_price = parameters["chainParameter"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["key"] == "getEnergyFee"))
        .and_then(|item| item["value"].as_u64())
        .ok_or("Tron energy price unavailable")?;
    let fee_limit = energy
        .checked_mul(energy_price)
        .and_then(|fee| fee.checked_mul(120))
        .map(|fee| fee / 100)
        .and_then(|fee| fee.checked_add(1_000_000))
        .ok_or("Tron fee estimate overflow")?;
    let response = rpc_post(
        client,
        &format!("{}/wallet/triggersmartcontract", tron_rpc_url()?),
        &serde_json::json!({
            "owner_address": from_hex,
            "contract_address": contract_hex,
            "function_selector": "transfer(address,uint256)",
            "parameter": parameter,
            "fee_limit": fee_limit,
            "call_value": 0,
            "visible": false
        }),
    )
    .await?;
    if response["result"]["result"].as_bool() != Some(true) {
        return Err(response["result"]["message"]
            .as_str()
            .map(|s| String::from_utf8_lossy(&hex::decode(s).unwrap_or_default()).into_owned())
            .unwrap_or_else(|| "Tron TRC-20 transaction construction failed".into()));
    }
    let transaction = response
        .get("transaction")
        .cloned()
        .ok_or("Tron TRC-20 transaction missing")?;
    Ok((transaction, fee_limit))
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
        crate::providers::fetch_tx_observation(client, "tron", txid).await?,
    ))
}

pub(crate) async fn fetch_tron_tx_observation(
    client: &reqwest::Client,
    config: &NetworkConfig,
    txid: &str,
) -> Result<Option<TxStatus>, String> {
    let json = rpc_post(
        client,
        &format!("{}/wallet/gettransactioninfobyid", config.rpc_url()?),
        &serde_json::json!({ "value": txid }),
    )
    .await?;

    let Some(observation) = parse_tron_tx_observation(&json)? else {
        return Ok(None);
    };
    let solid = rpc_post(
        client,
        &format!(
            "{}/walletsolidity/gettransactioninfobyid",
            config.rpc_url()?
        ),
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
