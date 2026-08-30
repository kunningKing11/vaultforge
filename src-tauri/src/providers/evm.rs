use crate::assets::cached_asset;
use crate::dto::Asset;
use crate::providers::NetworkAssetRefresh;
use crate::providers::http::rpc_post;
use crate::registry::{AssetConfig, NetworkConfig, network_by_id};

pub(crate) type EvmNetworkConfig = NetworkConfig;
pub(crate) type EvmTokenConfig = AssetConfig;

pub(crate) struct EvmFeeEstimate {
    pub(crate) max_priority_fee_per_gas: u128,
    pub(crate) max_fee_per_gas: u128,
}

pub(crate) fn evm_tokens_for_network(network_id: &str) -> &[EvmTokenConfig] {
    network_by_id(network_id)
        .filter(|network| network.kind == "evm")
        .map(|network| network.tokens.as_slice())
        .unwrap_or(&[])
}

pub(crate) fn evm_config_by_id(network_id: &str) -> Option<&'static EvmNetworkConfig> {
    network_by_id(network_id).filter(|network| network.kind == "evm")
}

pub(crate) async fn fetch_evm_native_balance(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    address: &str,
) -> Result<u128, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getBalance",
        "params": [address, "latest"],
        "id": 1,
    });

    let json = rpc_post(client, config.rpc_url()?, &body).await?;
    let balance_hex = json["result"]
        .as_str()
        .ok_or_else(|| "RPC response missing result field".to_string())?;

    u128::from_str_radix(balance_hex.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid balance hex: {e}"))
}

pub(crate) async fn fetch_evm_token_balance(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    token: &EvmTokenConfig,
    address: &str,
) -> Result<u128, String> {
    let addr_hex = address.trim_start_matches("0x");
    let addr_bytes = hex::decode(addr_hex).map_err(|_| "Invalid address".to_string())?;
    let mut padded = vec![0u8; 32];
    padded[32 - addr_bytes.len()..].copy_from_slice(&addr_bytes);

    let mut data = vec![0x70, 0xa0, 0x82, 0x31]; // keccak256("balanceOf(address)")[..4]
    data.extend_from_slice(&padded);

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{
            "to": token.token_address.as_deref().ok_or_else(|| "ERC-20 token contract is missing".to_string())?,
            "data": format!("0x{}", hex::encode(&data))
        }, "latest"],
        "id": 1,
    });

    let json = rpc_post(client, config.rpc_url()?, &body).await?;
    let hex_str = json["result"]
        .as_str()
        .ok_or_else(|| "Token balance RPC missing result".to_string())?;

    u128::from_str_radix(hex_str.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid token balance hex: {e}"))
}

pub(crate) async fn fetch_evm_assets(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    address: &str,
    cached_assets: &[Asset],
) -> NetworkAssetRefresh {
    let mut balance_failed = false;
    let mut assets = vec![];

    match fetch_evm_native_balance(client, config, address).await {
        Ok(wei) => assets.push(Asset {
            symbol: config.native_asset.symbol.clone(),
            name: config.native_asset.name.clone(),
            balance: wei.to_string(),
            decimals: config.native_asset.decimals,
            price_usd: 0.0,
            change_24h: 0.0,
            network: config.id.to_string(),
            token_address: None,
        }),
        Err(_) => {
            balance_failed = true;
            if let Some(cached) =
                cached_asset(cached_assets, &config.id, &config.native_asset.symbol)
            {
                assets.push(cached);
            }
        }
    }

    for token in evm_tokens_for_network(&config.id) {
        match fetch_evm_token_balance(client, config, token, address).await {
            Ok(balance) => {
                assets.push(Asset {
                    symbol: token.symbol.to_string(),
                    name: token.name.to_string(),
                    balance: balance.to_string(),
                    decimals: token.decimals,
                    price_usd: 0.0,
                    change_24h: 0.0,
                    network: config.id.to_string(),
                    token_address: token.token_address.clone(),
                });
            }
            Err(_) => {
                balance_failed = true;
                if let Some(cached) = cached_asset(cached_assets, &config.id, &token.symbol) {
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

pub(crate) async fn fetch_evm_nonce(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    address: &str,
) -> Result<u64, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionCount",
        "params": [address, "pending"],
        "id": 1,
    });
    let json = rpc_post(client, config.rpc_url()?, &body).await?;
    let hex_str = json["result"]
        .as_str()
        .ok_or_else(|| "Nonce RPC missing result".to_string())?;
    u64::from_str_radix(hex_str.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid nonce hex: {e}"))
}

// Fallback function if the RPC does not support eth_feeHistory, which is used to estimate EIP-1559 fees.
pub(crate) async fn fetch_evm_gas_price(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
) -> Result<u128, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_gasPrice",
        "params": [],
        "id": 1,
    });
    let json = rpc_post(client, config.rpc_url()?, &body).await?;
    let hex_str = json["result"]
        .as_str()
        .ok_or_else(|| "Gas price RPC missing result".to_string())?;
    u128::from_str_radix(hex_str.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid gas price hex: {e}"))
}

pub(crate) async fn fetch_evm_fee_estimate(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
) -> Result<EvmFeeEstimate, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_feeHistory",
        "params": ["0x5", "latest", [50]],
        "id": 1,
    });

    match rpc_post(client, config.rpc_url()?, &body).await {
        Ok(json) => parse_evm_fee_history(&json),
        Err(_) => {
            let gas_price = fetch_evm_gas_price(client, config).await?;
            Ok(EvmFeeEstimate {
                max_priority_fee_per_gas: gas_price,
                max_fee_per_gas: gas_price,
            })
        }
    }
}

pub(crate) fn parse_evm_fee_history(json: &serde_json::Value) -> Result<EvmFeeEstimate, String> {
    if let Some(error) = json.get("error") {
        return Err(format!("EVM fee history RPC error: {error}"));
    }

    let base_fees = json["result"]["baseFeePerGas"]
        .as_array()
        .ok_or_else(|| "EVM fee history missing baseFeePerGas".to_string())?;
    let latest_base_fee_hex = base_fees
        .last()
        .and_then(|value| value.as_str())
        .ok_or_else(|| "EVM fee history missing latest base fee".to_string())?;
    let base_fee = u128::from_str_radix(latest_base_fee_hex.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid EVM base fee hex: {e}"))?;

    let priority_fee = json["result"]["reward"]
        .as_array()
        .and_then(|rewards| rewards.last())
        .and_then(|last_reward| last_reward.as_array())
        .and_then(|percentiles| percentiles.first())
        .and_then(|value| value.as_str())
        .and_then(|hex| u128::from_str_radix(hex.trim_start_matches("0x"), 16).ok())
        .unwrap_or(1_500_000_000);

    let doubled_base_fee = base_fee
        .checked_mul(2)
        .ok_or_else(|| "EVM base fee estimate is too large".to_string())?;
    let max_fee_per_gas = doubled_base_fee
        .checked_add(priority_fee)
        .ok_or_else(|| "EVM max fee estimate is too large".to_string())?;

    Ok(EvmFeeEstimate {
        max_priority_fee_per_gas: priority_fee,
        max_fee_per_gas,
    })
}

pub(crate) async fn fetch_evm_estimated_gas(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    from: &str,
    to: &str,
    value: u128,
    data: &[u8],
) -> Result<u64, String> {
    let mut params = serde_json::json!({
        "from": from,
        "to": to,
        "value": format!("0x{:x}", value),
    });
    if !data.is_empty() {
        params["data"] = serde_json::Value::String(format!("0x{}", hex::encode(data)));
    }
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_estimateGas",
        "params": [params],
        "id": 1,
    });
    let json = rpc_post(client, config.rpc_url()?, &body).await?;
    let hex_str = json["result"]
        .as_str()
        .ok_or_else(|| "Estimate gas RPC missing result".to_string())?;
    u64::from_str_radix(hex_str.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid gas estimate hex: {e}"))
}

pub(crate) async fn broadcast_evm_tx(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    raw_tx_hex: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_sendRawTransaction",
        "params": [raw_tx_hex],
        "id": 1,
    });
    let json = rpc_post(client, config.rpc_url()?, &body).await?;
    json["result"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| {
            json["error"]["message"]
                .as_str()
                .unwrap_or("Unknown broadcast error")
                .to_string()
        })
}

pub(crate) async fn fetch_evm_tx_status(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    tx_hash: &str,
) -> Result<Option<String>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash],
        "id": 1,
    });
    let json = rpc_post(client, config.rpc_url()?, &body).await?;

    if json["result"].is_null() {
        return Ok(None);
    }

    let status_hex = json["result"]["status"].as_str().unwrap_or("0x0");
    if status_hex == "0x1" {
        Ok(Some("confirmed".to_string()))
    } else {
        Ok(Some("failed".to_string()))
    }
}
