use crate::assets::cached_asset;
use crate::dto::Asset;
use crate::providers::NetworkAssetRefresh;
use crate::providers::http::{json_rpc_result, optional_tx_hash, rpc_post_network};
use crate::registry::{AssetConfig, NetworkConfig, network_by_id};
use crate::tx::evm::{EvmCall, evm_address_word, evm_uint_word};
use crate::tx::tx_status::{Execution, Finality, TxStatus, legacy_tx_status};

pub(crate) type EvmNetworkConfig = NetworkConfig;
pub(crate) type EvmTokenConfig = AssetConfig;

#[derive(Debug)]
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

    let json = rpc_post_network(client, &config.id, config.rpc_url()?, &body).await?;
    parse_evm_balance(&json, "EVM balance")
}

pub(crate) async fn fetch_evm_token_balance(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    token: &EvmTokenConfig,
    address: &str,
) -> Result<u128, String> {
    let token_address = token
        .token_address
        .as_deref()
        .ok_or_else(|| "ERC-20 token contract is missing".to_string())?;
    fetch_evm_token_balance_for_contract(client, config, token_address, address).await
}

pub(crate) async fn fetch_evm_token_balance_for_contract(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    token_address: &str,
    address: &str,
) -> Result<u128, String> {
    crate::address::evm::validate_address(token_address)?;
    let mut data = vec![0x70, 0xa0, 0x82, 0x31];
    data.extend_from_slice(&evm_address_word(address)?);

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{
            "to": token_address,
            "data": format!("0x{}", hex::encode(&data))
        }, "latest"],
        "id": 1,
    });

    let json = rpc_post_network(client, &config.id, config.rpc_url()?, &body).await?;
    parse_evm_balance(&json, "EVM token balance")
}

/// Keep the full uint256: common unlimited approvals exceed u128.
#[derive(Debug)]
pub(crate) struct EvmAllowance([u8; 32]);

impl EvmAllowance {
    pub(crate) fn covers(&self, amount: u128) -> bool {
        self.0 >= evm_uint_word(amount)
    }
}

pub(crate) async fn fetch_evm_allowance(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    token: &str,
    owner: &str,
    spender: &str,
) -> Result<EvmAllowance, String> {
    crate::address::evm::validate_address(token)?;
    let mut data = vec![0xdd, 0x62, 0xed, 0x3e];
    data.extend_from_slice(&evm_address_word(owner)?);
    data.extend_from_slice(&evm_address_word(spender)?);
    let json = rpc_post_network(
        client,
        &config.id,
        config.rpc_url()?,
        &serde_json::json!({
            "jsonrpc": "2.0", "method": "eth_call", "id": 1,
            "params": [{"to": token, "data": format!("0x{}", hex::encode(data))}, "latest"],
        }),
    )
    .await?;
    let result = json_rpc_result(&json, "EVM allowance")?
        .as_str()
        .ok_or("Invalid EVM allowance result")?;
    let bytes = hex::decode(
        result
            .strip_prefix("0x")
            .ok_or("Invalid EVM allowance prefix")?,
    )
    .map_err(|_| "Invalid EVM allowance hex")?;
    Ok(EvmAllowance(
        bytes
            .try_into()
            .map_err(|_| "EVM allowance must be a uint256 ABI word")?,
    ))
}

/// Checks endpoint identity before preparing a chain-specific contract call.
pub(crate) async fn verify_evm_chain(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
) -> Result<(), String> {
    let json = rpc_post_network(
        client,
        &config.id,
        config.rpc_url()?,
        &serde_json::json!({"jsonrpc":"2.0", "method":"eth_chainId", "params":[], "id":1}),
    )
    .await?;
    let chain = json_rpc_result(&json, "EVM chain ID")?
        .as_str()
        .ok_or("Invalid EVM chain ID")?;
    let chain = u64::from_str_radix(
        chain
            .strip_prefix("0x")
            .ok_or("Invalid EVM chain ID prefix")?,
        16,
    )
    .map_err(|_| "Invalid EVM chain ID")?;
    if chain != config.chain_id()? {
        return Err("EVM RPC chain ID does not match configured deployment".into());
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct EvmCallEstimate {
    pub(crate) gas_limit: u64,
    pub(crate) fees: EvmFeeEstimate,
    pub(crate) max_native_fee: u128,
}

pub(crate) fn evm_max_native_fee(gas_limit: u64, max_fee_per_gas: u128) -> Result<u128, String> {
    if gas_limit == 0 || max_fee_per_gas == 0 {
        return Err("EVM gas and fee estimates must be positive".into());
    }
    (gas_limit as u128)
        .checked_mul(max_fee_per_gas)
        .ok_or_else(|| "EVM maximum fee overflows base units".into())
}

/// ~15% headroom on the EIP-1559 fee cap so a signed transaction survives
/// modest base-fee movement between preparation and submission. The cap is a
/// worst case, not the paid price, so this never spends more on its own.
/// Integer math only; rounds up so dust estimates keep at least +1 wei.
pub(crate) fn with_fee_headroom(max_fee_per_gas: u128) -> Result<u128, String> {
    const NUMERATOR: u128 = 23;
    const DENOMINATOR: u128 = 20;
    max_fee_per_gas
        .checked_mul(NUMERATOR)
        .and_then(|scaled| scaled.checked_add(DENOMINATOR - 1))
        .map(|scaled| scaled / DENOMINATOR)
        .filter(|capped| *capped >= max_fee_per_gas)
        .ok_or_else(|| "EVM fee headroom overflows base units".into())
}

/// Recomputes the spending ceiling after headroom so balance checks, saved
/// contexts, and displayed fees all cover the same worst case that gets signed.
pub(crate) fn estimate_with_fee_headroom(
    estimate: EvmCallEstimate,
) -> Result<EvmCallEstimate, String> {
    let max_fee_per_gas = with_fee_headroom(estimate.fees.max_fee_per_gas)?;
    let max_native_fee = evm_max_native_fee(estimate.gas_limit, max_fee_per_gas)?;
    Ok(EvmCallEstimate {
        gas_limit: estimate.gas_limit,
        fees: EvmFeeEstimate {
            max_priority_fee_per_gas: estimate.fees.max_priority_fee_per_gas,
            max_fee_per_gas,
        },
        max_native_fee,
    })
}

/// Shared preparation seam for native/token sends and supported contract calls.
pub(crate) async fn estimate_evm_call(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    from: &str,
    call: &EvmCall,
) -> Result<EvmCallEstimate, String> {
    crate::address::evm::validate_address(from)?;
    crate::address::evm::validate_address(&call.to)?;
    let fees = fetch_evm_fee_estimate(client, config).await?;
    let gas_limit =
        fetch_evm_estimated_gas(client, config, from, &call.to, call.value, &call.data).await?;
    let max_native_fee = evm_max_native_fee(gas_limit, fees.max_fee_per_gas)?;
    Ok(EvmCallEstimate {
        gas_limit,
        fees,
        max_native_fee,
    })
}

fn parse_evm_balance(json: &serde_json::Value, context: &str) -> Result<u128, String> {
    let balance_hex = json_rpc_result(json, context)?
        .as_str()
        .ok_or_else(|| format!("{context} RPC result is not a hex quantity"))?;
    u128::from_str_radix(balance_hex.trim_start_matches("0x"), 16)
        .map_err(|_| format!("Invalid {context} hex quantity"))
}

/// Fetches native and token balances, reusing cached values on RPC failure.
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
            solana_token: None,
            symbol: config.native_asset.symbol.clone(),
            unicode_symbol: config.native_asset.unicode_symbol.clone(),
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
            if let Some(cached) = cached_asset(cached_assets, &config.id, None) {
                assets.push(cached);
            }
        }
    }

    for token in evm_tokens_for_network(&config.id) {
        match fetch_evm_token_balance(client, config, token, address).await {
            Ok(balance) => {
                assets.push(Asset {
                    solana_token: None,
                    symbol: token.symbol.to_string(),
                    unicode_symbol: token.unicode_symbol.clone(),
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
                if let Some(cached) =
                    cached_asset(cached_assets, &config.id, token.token_address.as_deref())
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

/// Fetches the account's pending nonce without reserving it locally.
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
    let json = rpc_post_network(client, &config.id, config.rpc_url()?, &body).await?;
    let hex_str = json_rpc_result(&json, "EVM nonce")?
        .as_str()
        .ok_or_else(|| "EVM nonce RPC result is not a hex quantity".to_string())?;
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
    let json = rpc_post_network(client, &config.id, config.rpc_url()?, &body).await?;
    let hex_str = json_rpc_result(&json, "EVM gas price")?
        .as_str()
        .ok_or_else(|| "EVM gas price RPC result is not a hex quantity".to_string())?;
    u128::from_str_radix(hex_str.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid gas price hex: {e}"))
}

/// Estimates EIP-1559 fees, falling back to `gasPrice` if `feeHistory` fails.
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

    match rpc_post_network(client, &config.id, config.rpc_url()?, &body).await {
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

/// Calculates max fee from twice the latest base fee plus median priority tip.
pub(crate) fn parse_evm_fee_history(json: &serde_json::Value) -> Result<EvmFeeEstimate, String> {
    let result = json_rpc_result(json, "EVM fee history")?;
    let base_fees = result["baseFeePerGas"]
        .as_array()
        .ok_or_else(|| "EVM fee history missing baseFeePerGas".to_string())?;
    let latest_base_fee_hex = base_fees
        .last()
        .and_then(|value| value.as_str())
        .ok_or_else(|| "EVM fee history missing latest base fee".to_string())?;
    let base_fee = u128::from_str_radix(latest_base_fee_hex.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid EVM base fee hex: {e}"))?;

    let priority_fee = result["reward"]
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
    let json = rpc_post_network(client, &config.id, config.rpc_url()?, &body).await?;
    let hex_str = json_rpc_result(&json, "EVM gas estimate")?
        .as_str()
        .ok_or_else(|| "EVM gas estimate RPC result is not a hex quantity".to_string())?;
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
    let json = rpc_post_network(client, &config.id, config.rpc_url()?, &body).await?;
    json_rpc_result(&json, "EVM broadcast")?
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "EVM broadcast RPC result is not a transaction hash".to_string())
}

pub(crate) async fn fetch_evm_tx_status(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    tx_hash: &str,
) -> Result<Option<String>, String> {
    let observation = crate::providers::fetch_tx_observation(client, &config.id, tx_hash).await?;

    Ok(legacy_tx_status(observation))
}

/// Read canonical inclusion promptly, without waiting on the finalized-head RPC.
/// This reports receipt success only; it never asserts transaction finality.
pub(crate) async fn fetch_evm_tx_inclusion(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    tx_hash: &str,
) -> Result<Option<TxStatus>, String> {
    let Some(observation) = fetch_evm_receipt(client, config, tx_hash).await? else {
        return Ok(None);
    };
    let height = observation
        .block_height
        .ok_or("Missing EVM receipt height")?;
    let body = serde_json::json!({"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":[format!("0x{height:x}"),false],"id":2});
    let canonical = rpc_post_network(client, &config.id, config.rpc_url()?, &body).await?;
    let canonical = json_rpc_result(&canonical, "EVM canonical block")?;
    finalize_evm_observation(observation, canonical, &serde_json::Value::Null)
}

async fn fetch_evm_receipt(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    tx_hash: &str,
) -> Result<Option<TxStatus>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash],
        "id": 1,
    });
    let json = rpc_post_network(client, &config.id, config.rpc_url()?, &body).await?;

    let result = json_rpc_result(&json, "EVM transaction status")?;

    parse_evm_tx_receipt(result)
}

pub(crate) async fn fetch_evm_tx_observation(
    client: &reqwest::Client,
    config: &EvmNetworkConfig,
    tx_hash: &str,
) -> Result<Option<TxStatus>, String> {
    let Some(observation) = fetch_evm_receipt(client, config, tx_hash).await? else {
        return Ok(None);
    };
    let height = observation
        .block_height
        .ok_or_else(|| "Missing EVM receipt height".to_string())?;
    let finalized = match rpc_post_network(client, &config.id, config.rpc_url()?, &serde_json::json!({"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["finalized",false],"id":3})).await {
        Ok(json) => json.get("result").cloned().unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    };
    // Check canonical inclusion after reading finality so an intervening reorg cannot finalize an old fork.
    let canonical = rpc_post_network(client, &config.id, config.rpc_url()?, &serde_json::json!({"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":[format!("0x{height:x}"),false],"id":2})).await?;
    let canonical = json_rpc_result(&canonical, "EVM canonical block")?;
    finalize_evm_observation(observation, canonical, &finalized)
}

pub(crate) fn finalize_evm_observation(
    mut observation: TxStatus,
    canonical: &serde_json::Value,
    finalized: &serde_json::Value,
) -> Result<Option<TxStatus>, String> {
    if canonical.is_null() {
        return Ok(None);
    }
    let height = observation
        .block_height
        .ok_or_else(|| "Missing EVM receipt height".to_string())?;
    let canonical_height = parse_evm_block_height(canonical, "number")?;
    let canonical_hash = optional_tx_hash(canonical, "hash", "0x")?
        .ok_or_else(|| "Missing EVM canonical block hash".to_string())?;
    if canonical_height != height
        || !observation
            .block_hash
            .as_deref()
            .is_some_and(|hash| hash.eq_ignore_ascii_case(&canonical_hash))
    {
        return Ok(None);
    }
    if !finalized.is_null() {
        let finalized_height = parse_evm_block_height(finalized, "number")?;
        let finalized_hash = optional_tx_hash(finalized, "hash", "0x")?
            .ok_or_else(|| "Missing EVM finalized block hash".to_string())?;
        if finalized_height >= height {
            if finalized_height == height && !finalized_hash.eq_ignore_ascii_case(&canonical_hash) {
                return Err("EVM finalized head conflicts with canonical receipt block".to_string());
            }
            observation.finality = Finality::Finalized;
        }
    }
    Ok(Some(observation))
}

fn parse_evm_block_height(result: &serde_json::Value, field: &str) -> Result<u64, String> {
    let quantity = result[field]
        .as_str()
        .and_then(|number| number.strip_prefix("0x"))
        .ok_or_else(|| format!("Missing or invalid EVM {field}"))?;
    if quantity.is_empty()
        || !quantity.bytes().all(|byte| byte.is_ascii_hexdigit())
        || (quantity.len() > 1 && quantity.starts_with('0'))
    {
        return Err(format!("Invalid EVM {field}"));
    }
    u64::from_str_radix(quantity, 16).map_err(|_| format!("Invalid EVM {field}"))
}

pub(crate) fn parse_evm_tx_receipt(result: &serde_json::Value) -> Result<Option<TxStatus>, String> {
    if result.is_null() {
        return Ok(None);
    }

    let execution = match result["status"].as_str() {
        Some("0x1") => Execution::Succeeded,
        Some("0x0") => Execution::Failed,
        _ => return Err("Invalid EVM receipt status".to_string()),
    };

    let block_height = parse_evm_block_height(result, "blockNumber")?;

    let block_hash = optional_tx_hash(result, "blockHash", "0x")?
        .ok_or_else(|| "Missing EVM receipt block hash".to_string())?;

    let mut observation = TxStatus::new(execution, Finality::Included);
    observation.block_height = Some(block_height);
    observation.block_hash = Some(block_hash);
    Ok(Some(observation))
}

#[cfg(test)]
#[path = "../tests/providers/evm.rs"]
mod tests;
