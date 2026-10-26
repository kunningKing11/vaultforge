use crate::assets::{cached_asset, cached_asset_by_token_address};
use crate::dto::Asset;
use crate::providers::NetworkAssetRefresh;
use crate::providers::http::{json_rpc_result, optional_tx_height, rpc_post};
use crate::registry::{NetworkConfig, configured_asset, network_by_id};
use crate::tx::tx_status::{Execution, Finality, TxStatus, legacy_tx_status};
use std::collections::BTreeMap;

const SOLANA_TOKEN_PROGRAM_ID: &str = crate::solana_tokens::CLASSIC;

fn solana_config() -> Result<&'static NetworkConfig, String> {
    network_by_id("solana").ok_or_else(|| "Solana is missing from the network registry".to_string())
}

fn solana_rpc_url() -> Result<&'static str, String> {
    solana_config()?.rpc_url()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SolanaTokenAccount {
    pub(crate) capabilities: Option<crate::dto::SolanaTokenCapabilities>,
    pub(crate) address: String,
    pub(crate) mint: String,
    pub(crate) amount: u64,
    pub(crate) decimals: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SolanaTokenAccountState {
    pub(crate) amount: u64,
    pub(crate) decimals: u8,
}

pub(crate) async fn fetch_solana_native_balance(
    client: &reqwest::Client,
    address: &str,
) -> Result<u128, String> {
    fetch_solana_native_balance_at(client, solana_rpc_url()?, address).await
}

async fn fetch_solana_native_balance_at(
    client: &reqwest::Client,
    url: &str,
    address: &str,
) -> Result<u128, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getBalance",
        "params": [address],
        "id": 1,
    });
    let json = rpc_post(client, url, &body).await?;
    parse_solana_balance(&json)
}

pub(crate) async fn fetch_solana_token_accounts(
    client: &reqwest::Client,
    address: &str,
) -> Result<Vec<SolanaTokenAccount>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getTokenAccountsByOwner",
        "params": [address, {"programId": SOLANA_TOKEN_PROGRAM_ID}, {"encoding": "jsonParsed"}],
        "id": 1,
    });
    let json = rpc_post(client, solana_rpc_url()?, &body).await?;
    parse_solana_token_accounts(&json, address)
}

/// Portfolio discovery is separate from the classic-only send account reader.
/// Neither program's partial response can prove that an absent mint has zero funds.
async fn fetch_solana_portfolio_accounts_at(
    client: &reqwest::Client,
    url: &str,
    address: &str,
) -> Result<Vec<SolanaTokenAccount>, String> {
    fetch_solana_portfolio_accounts_with(address, |method, params| async move {
        rpc_post(
            client,
            url,
            &serde_json::json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
        )
        .await
    })
    .await
}

pub(crate) async fn fetch_solana_portfolio_accounts_with<F, Fut>(
    address: &str,
    mut call: F,
) -> Result<Vec<SolanaTokenAccount>, String>
where
    F: FnMut(&'static str, serde_json::Value) -> Fut,
    Fut: std::future::Future<Output = Result<serde_json::Value, String>>,
{
    use crate::solana_tokens::{CLASSIC, TOKEN_2022, inspect_holding, inspect_mint};
    let mut holdings = Vec::new();
    let mut addresses = std::collections::BTreeSet::new();
    for program in [CLASSIC, TOKEN_2022] {
        let response = call("getTokenAccountsByOwner",serde_json::json!([address,{"programId":program},{"encoding":"base64","commitment":"confirmed"}])).await?;
        let values = json_rpc_result(&response, "Solana portfolio token accounts")?["value"]
            .as_array()
            .ok_or("Solana portfolio account list missing")?;
        for value in values {
            let account = value["pubkey"]
                .as_str()
                .ok_or("Solana token account address missing")?;
            let holding = inspect_holding(account, address, &value["account"])?;
            if holding.program.id() != program || !addresses.insert(account.to_string()) {
                return Err(
                    "Solana token discovery returned a foreign program or duplicate account".into(),
                );
            }
            holdings.push(holding);
        }
    }
    let mints: Vec<_> = holdings
        .iter()
        .map(|h| h.mint.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut descriptors = BTreeMap::new();
    for chunk in mints.chunks(100) {
        let response = call(
            "getMultipleAccounts",
            serde_json::json!([chunk,{"encoding":"base64","commitment":"confirmed"}]),
        )
        .await?;
        let values = json_rpc_result(&response, "Solana portfolio mints")?["value"]
            .as_array()
            .filter(|v| v.len() == chunk.len())
            .ok_or("Solana mint response length mismatch")?;
        for (mint, value) in chunk.iter().zip(values) {
            // Closing a Token-2022 mint requires zero supply; its empty accounts
            // can remain. Nonzero holdings still require a descriptor below.
            if value.is_null() {
                continue;
            }
            descriptors.insert(mint.clone(), inspect_mint(mint, value)?);
        }
    }
    holdings
        .into_iter()
        .filter(|holding| holding.amount > 0 || descriptors.contains_key(&holding.mint))
        .map(|holding| {
            let descriptor = descriptors
                .get(&holding.mint)
                .ok_or("Solana mint descriptor missing")?;
            if holding.program != descriptor.program
                || descriptor
                    .required_account_extensions
                    .iter()
                    .any(|extension| !holding.extensions.contains(extension))
            {
                return Err(
                    "Solana token account program or required extensions differ from mint".into(),
                );
            }
            // Account.amount already excludes withheld transfer fees. Frozen and other
            // unsupported accounts remain visible; execution readers still reject them.
            let reason = descriptor
                .unsupported_reason()
                .or_else(|| holding.unsupported_reason());
            let capabilities = crate::dto::SolanaTokenCapabilities {
                program: descriptor.program.id().into(),
                extensions: descriptor
                    .extensions
                    .iter()
                    .copied()
                    .map(u16::from)
                    .collect(),
                send_supported: reason.is_none(),
                swap_supported: reason.is_none()
                    && holding.address == descriptor.associated_address(address)?,
                unsupported_reason: reason,
            };
            Ok(SolanaTokenAccount {
                capabilities: Some(capabilities),
                address: holding.address,
                mint: holding.mint,
                amount: holding.amount,
                decimals: descriptor.decimals,
            })
        })
        .collect()
}

/// Reads verified mint state for program-aware account derivation and future execution.
#[allow(dead_code)]
pub(crate) async fn fetch_solana_token_descriptor(
    client: &reqwest::Client,
    mint: &str,
) -> Result<crate::solana_tokens::TokenDescriptor, String> {
    let response = rpc_post(
        client,
        solana_rpc_url()?,
        &serde_json::json!({
            "jsonrpc":"2.0", "id":1, "method":"getAccountInfo",
            "params":[mint,{"encoding":"base64","commitment":"confirmed"}]
        }),
    )
    .await?;
    crate::solana_tokens::inspect_mint(
        mint,
        &json_rpc_result(&response, "Solana token mint")?["value"],
    )
}

/// Fetches native, classic SPL and Token-2022 balances; failures retain cached tokens.
pub(crate) async fn fetch_solana_assets(
    client: &reqwest::Client,
    address: &str,
    cached_assets: &[Asset],
) -> NetworkAssetRefresh {
    let url = solana_rpc_url().expect("Solana must have a configured RPC");
    fetch_solana_assets_at(client, url, address, cached_assets).await
}

async fn fetch_solana_assets_at(
    client: &reqwest::Client,
    url: &str,
    address: &str,
    cached_assets: &[Asset],
) -> NetworkAssetRefresh {
    let config = solana_config().expect("Solana must exist in the generated network registry");
    let mut balance_failed = false;
    let mut assets = vec![];

    match fetch_solana_native_balance_at(client, url, address).await {
        Ok(lamports) => assets.push(Asset {
            solana_token: None,
            symbol: config.native_asset.symbol.clone(),
            unicode_symbol: config.native_asset.unicode_symbol.clone(),
            name: config.native_asset.name.clone(),
            balance: lamports.to_string(),
            decimals: config.native_asset.decimals,
            price_usd: 0.0,
            change_24h: 0.0,
            network: "solana".to_string(),
            token_address: None,
        }),
        Err(_) => {
            balance_failed = true;
            if let Some(cached) = cached_asset(cached_assets, &config.id, None) {
                assets.push(cached);
            }
        }
    }

    let token_accounts = match fetch_solana_portfolio_accounts_at(client, url, address).await {
        Ok(accounts) => accounts,
        Err(_) => {
            balance_failed = true;
            assets.extend(cached_solana_token_assets(cached_assets));
            return NetworkAssetRefresh {
                assets,
                balance_failed,
            };
        }
    };

    assets.extend(solana_token_assets(config, token_accounts, cached_assets));
    NetworkAssetRefresh {
        assets,
        balance_failed,
    }
}

/// A complete successful owner-account read proves zero for absent registered mints.
pub(crate) fn solana_token_assets(
    config: &NetworkConfig,
    token_accounts: Vec<SolanaTokenAccount>,
    cached_assets: &[Asset],
) -> Vec<Asset> {
    let mut token_balances = BTreeMap::<String, (u128, u32)>::new();
    let mut capabilities = BTreeMap::<String, crate::dto::SolanaTokenCapabilities>::new();
    for account in token_accounts {
        if let Some(current) = account.capabilities {
            capabilities
                .entry(account.mint.clone())
                .and_modify(|previous| {
                    previous.send_supported |= current.send_supported;
                    previous.swap_supported |= current.swap_supported;
                    if previous.send_supported {
                        previous.unsupported_reason = None;
                    }
                })
                .or_insert(current);
        }
        if account.amount == 0 && configured_asset(config, Some(&account.mint)).is_none() {
            continue;
        }
        let entry = token_balances
            .entry(account.mint)
            .or_insert((0, u32::from(account.decimals)));
        if entry.1 == u32::from(account.decimals) {
            entry.0 += u128::from(account.amount);
        }
    }
    for token in &config.tokens {
        if let Some(mint) = &token.token_address {
            token_balances
                .entry(mint.clone())
                .or_insert((0, token.decimals));
        }
    }

    let mut assets = Vec::with_capacity(token_balances.len());
    for (mint, (amount, decimals)) in token_balances {
        let cached = cached_asset_by_token_address(cached_assets, "solana", &mint);
        let configured = configured_asset(config, Some(&mint));
        let symbol = configured
            .map(|asset| asset.symbol.clone())
            .or_else(|| cached.as_ref().map(|asset| asset.symbol.clone()))
            .unwrap_or_else(|| fallback_solana_token_symbol(&mint));
        let name = configured
            .map(|asset| asset.name.clone())
            .or_else(|| cached.as_ref().map(|asset| asset.name.clone()))
            .unwrap_or_else(|| format!("SPL Token {}", short_mint(&mint)));
        let unicode_symbol = configured.map_or_else(
            || {
                cached
                    .as_ref()
                    .and_then(|asset| asset.unicode_symbol.clone())
            },
            |asset| asset.unicode_symbol.clone(),
        );
        let price_usd = cached.as_ref().map(|asset| asset.price_usd).unwrap_or(0.0);
        let change_24h = cached.as_ref().map(|asset| asset.change_24h).unwrap_or(0.0);

        assets.push(Asset {
            solana_token: capabilities.remove(&mint),
            symbol,
            unicode_symbol,
            name,
            balance: amount.to_string(),
            decimals,
            price_usd,
            change_24h,
            network: "solana".to_string(),
            token_address: Some(mint),
        });
    }
    assets
}

fn cached_solana_token_assets(cached_assets: &[Asset]) -> impl Iterator<Item = Asset> + '_ {
    cached_assets
        .iter()
        .filter(|asset| asset.network == "solana" && asset.token_address.is_some())
        .cloned()
}

fn fallback_solana_token_symbol(mint: &str) -> String {
    format!("SPL-{}", short_mint(mint))
}

fn short_mint(mint: &str) -> String {
    mint.chars().take(6).collect()
}

pub(crate) fn parse_solana_balance(json: &serde_json::Value) -> Result<u128, String> {
    json_rpc_result(json, "Solana balance")?["value"]
        .as_u64()
        .map(|value| value as u128)
        .ok_or_else(|| "Solana balance RPC missing result.value".to_string())
}

pub(crate) fn parse_solana_token_accounts(
    json: &serde_json::Value,
    expected_owner: &str,
) -> Result<Vec<SolanaTokenAccount>, String> {
    let accounts = json_rpc_result(json, "Solana token accounts")?["value"]
        .as_array()
        .ok_or_else(|| "Solana token accounts RPC missing result.value".to_string())?;
    let mut parsed = Vec::new();

    for account in accounts {
        let address = account["pubkey"]
            .as_str()
            .ok_or_else(|| "Solana token account is missing its address".to_string())?;
        if account["account"]["owner"].as_str() != Some(SOLANA_TOKEN_PROGRAM_ID) {
            return Err(
                "Solana token account is not owned by the classic SPL Token program".to_string(),
            );
        }
        if account["account"]["data"]["parsed"]["type"].as_str() != Some("account") {
            return Err("Solana account is not a parsed SPL token account".to_string());
        }
        let info = &account["account"]["data"]["parsed"]["info"];
        let mint = info["mint"]
            .as_str()
            .ok_or_else(|| "Solana token account is missing its mint".to_string())?;
        if info["owner"].as_str() != Some(expected_owner) {
            return Err("Solana token account owner does not match the wallet".to_string());
        }
        if info["state"].as_str() != Some("initialized") {
            return Err("Solana token account is not initialized".to_string());
        }
        let token_amount = &info["tokenAmount"];
        let amount = token_amount["amount"]
            .as_str()
            .ok_or_else(|| "Solana token account is missing its base-unit balance".to_string())?
            .parse()
            .map_err(|_| "Solana token account has an invalid base-unit balance".to_string())?;
        let decimals = token_amount["decimals"]
            .as_u64()
            .ok_or_else(|| "Solana token account is missing decimals".to_string())?
            .try_into()
            .map_err(|_| "Solana token account decimals are too large".to_string())?;
        parsed.push(SolanaTokenAccount {
            capabilities: None,
            address: address.to_string(),
            mint: mint.to_string(),
            amount,
            decimals,
        });
    }

    Ok(parsed)
}

pub(crate) async fn broadcast_solana_transaction(
    client: &reqwest::Client,
    raw_tx_base64: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "sendTransaction",
        "params": [raw_tx_base64, {"encoding": "base64"}],
        "id": 1,
    });
    let json = rpc_post(client, solana_rpc_url()?, &body).await?;
    json_rpc_result(&json, "Solana broadcast")?
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Solana broadcast RPC result is not a transaction signature".to_string())
}

pub(crate) async fn fetch_solana_tx_status(
    client: &reqwest::Client,
    signature: &str,
) -> Result<Option<String>, String> {
    Ok(legacy_tx_status(
        crate::providers::fetch_tx_observation(client, "solana", signature).await?,
    ))
}

pub(crate) async fn fetch_solana_tx_observation(
    client: &reqwest::Client,
    config: &NetworkConfig,
    signature: &str,
) -> Result<Option<TxStatus>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getSignatureStatuses",
        "params": [[signature], {"searchTransactionHistory": true}],
        "id": 1,
    });
    let json = rpc_post(client, config.rpc_url()?, &body).await?;
    parse_solana_tx_observation(&json)
}

pub(crate) async fn fetch_solana_token_account_state(
    client: &reqwest::Client,
    ata_address: &str,
    expected_owner: &str,
    expected_mint: &str,
) -> Result<Option<SolanaTokenAccountState>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [ata_address, {"encoding": "jsonParsed"}],
        "id": 1,
    });
    let json = rpc_post(client, solana_rpc_url()?, &body).await?;
    parse_solana_token_account_state(&json, expected_owner, expected_mint)
}

pub(crate) async fn fetch_solana_mint_decimals(
    client: &reqwest::Client,
    mint: &str,
) -> Result<u8, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [mint, {"encoding": "jsonParsed"}],
        "id": 1,
    });
    let json = rpc_post(client, solana_rpc_url()?, &body).await?;
    parse_solana_mint_decimals(&json)
}

pub(crate) fn parse_solana_token_account_state(
    json: &serde_json::Value,
    expected_owner: &str,
    expected_mint: &str,
) -> Result<Option<SolanaTokenAccountState>, String> {
    let value = &json_rpc_result(json, "Solana account info")?["value"];
    if value.is_null() {
        return Ok(None);
    }

    let program_owner = value["owner"]
        .as_str()
        .ok_or_else(|| "Solana token account missing owner program".to_string())?;
    if program_owner != SOLANA_TOKEN_PROGRAM_ID {
        return Err("Existing Solana token account is not owned by SPL Token program".to_string());
    }

    let info = &value["data"]["parsed"]["info"];
    if value["data"]["parsed"]["type"].as_str() != Some("account") {
        return Err("Solana account is not a parsed SPL token account".to_string());
    }

    let mint = info["mint"]
        .as_str()
        .ok_or_else(|| "Solana token account missing mint".to_string())?;
    if mint != expected_mint {
        return Err("Existing Solana token account mint does not match transfer mint".to_string());
    }

    let owner = info["owner"]
        .as_str()
        .ok_or_else(|| "Solana token account missing owner".to_string())?;
    if owner != expected_owner {
        return Err("Existing Solana token account owner does not match recipient".to_string());
    }

    if info["state"].as_str() != Some("initialized") {
        return Err("Solana token account is not initialized".to_string());
    }

    let amount = info["tokenAmount"]["amount"]
        .as_str()
        .ok_or_else(|| "Solana token account missing base-unit balance".to_string())?
        .parse()
        .map_err(|_| "Solana token account has an invalid base-unit balance".to_string())?;
    let decimals = info["tokenAmount"]["decimals"]
        .as_u64()
        .ok_or_else(|| "Solana token account missing decimals".to_string())?
        .try_into()
        .map_err(|_| "Solana token account decimals are too large".to_string())?;

    Ok(Some(SolanaTokenAccountState { amount, decimals }))
}

pub(crate) fn parse_solana_mint_decimals(json: &serde_json::Value) -> Result<u8, String> {
    let value = &json_rpc_result(json, "Solana mint info")?["value"];
    if value.is_null() {
        return Err("Solana token mint account does not exist".to_string());
    }
    if value["owner"].as_str() != Some(SOLANA_TOKEN_PROGRAM_ID) {
        return Err("Solana token mint is not owned by the classic SPL Token program".to_string());
    }
    if value["data"]["parsed"]["type"].as_str() != Some("mint") {
        return Err("Solana account is not a parsed SPL token mint".to_string());
    }

    value["data"]["parsed"]["info"]["decimals"]
        .as_u64()
        .ok_or_else(|| "Solana token mint missing decimals".to_string())?
        .try_into()
        .map_err(|_| "Solana token mint decimals are too large".to_string())
}

pub(crate) async fn simulate_solana_transaction(
    client: &reqwest::Client,
    raw_tx_base64: &str,
) -> Result<(), String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "simulateTransaction",
        "params": [raw_tx_base64, {
            "encoding": "base64",
            "sigVerify": true,
            "replaceRecentBlockhash": false,
            "commitment": "processed",
        }],
        "id": 1,
    });
    let json = rpc_post(client, solana_rpc_url()?, &body).await?;
    parse_solana_simulation(&json)
}

pub(crate) fn parse_solana_simulation(json: &serde_json::Value) -> Result<(), String> {
    let value = json_rpc_result(json, "Solana simulation")?
        .get("value")
        .ok_or_else(|| "Solana simulation RPC missing result.value".to_string())?;
    if value["err"].is_null() {
        return Ok(());
    }
    Err(format!(
        "Solana transaction simulation failed: {}",
        value["err"]
    ))
}

pub(crate) async fn fetch_solana_token_account_rent(
    client: &reqwest::Client,
) -> Result<u64, String> {
    fetch_solana_token_account_rent_for_size(client, 165).await
}

/// Includes mint-required Token-2022 extensions and ATA immutable ownership.
#[allow(dead_code)]
pub(crate) async fn fetch_solana_mint_account_rent(
    client: &reqwest::Client,
    descriptor: &crate::solana_tokens::TokenDescriptor,
) -> Result<u64, String> {
    fetch_solana_token_account_rent_for_size(client, descriptor.account_len()?).await
}

async fn fetch_solana_token_account_rent_for_size(
    client: &reqwest::Client,
    account_size: usize,
) -> Result<u64, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getMinimumBalanceForRentExemption",
        "params": [account_size],
        "id": 1,
    });
    let json = rpc_post(client, solana_rpc_url()?, &body).await?;
    parse_solana_rent_exemption(&json)
}

pub(crate) fn parse_solana_rent_exemption(json: &serde_json::Value) -> Result<u64, String> {
    json_rpc_result(json, "Solana rent exemption")?
        .as_u64()
        .ok_or_else(|| "Solana rent exemption RPC missing result".to_string())
}

/// Fetches the latest finalized Solana blockhash for transaction signing.
pub(crate) async fn fetch_latest_solana_blockhash(
    client: &reqwest::Client,
) -> Result<String, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getLatestBlockhash",
        "params": [{"commitment": "finalized"}],
        "id": 1,
    });
    let json = rpc_post(client, solana_rpc_url()?, &body).await?;
    parse_latest_solana_blockhash(&json)
}

pub(crate) fn parse_latest_solana_blockhash(json: &serde_json::Value) -> Result<String, String> {
    json_rpc_result(json, "Solana blockhash")?["value"]["blockhash"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Solana blockhash RPC missing result.value.blockhash".to_string())
}

pub(crate) fn parse_solana_tx_status(json: &serde_json::Value) -> Result<Option<String>, String> {
    Ok(legacy_tx_status(parse_solana_tx_observation(json)?))
}

pub(crate) fn parse_solana_tx_observation(
    json: &serde_json::Value,
) -> Result<Option<TxStatus>, String> {
    let status = json_rpc_result(json, "Solana status")?["value"]
        .as_array()
        .and_then(|items| items.first())
        .ok_or_else(|| "Invalid Solana signature status response".to_string())?;
    if status.is_null() {
        return Ok(None);
    }
    let finality = match status["confirmationStatus"].as_str() {
        Some("processed" | "confirmed") => Finality::Included,
        Some("finalized") => Finality::Finalized,
        _ => return Err("Invalid Solana confirmation status".to_string()),
    };
    let execution = match status.get("err") {
        Some(serde_json::Value::Null) => Execution::Succeeded,
        Some(error) if error.is_object() || error.is_string() => Execution::Failed,
        _ => return Err("Invalid Solana transaction error field".to_string()),
    };
    let mut observation = TxStatus::new(execution, finality);
    observation.slot = Some(
        optional_tx_height(status, "slot")?
            .ok_or_else(|| "Missing Solana transaction slot".to_string())?,
    );
    observation.confirmations = optional_tx_height(status, "confirmations")?;
    Ok(Some(observation))
}

#[allow(dead_code)]
pub(crate) async fn fetch_solana_fee_for_message(
    client: &reqwest::Client,
    message_base64: &str,
) -> Result<u64, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getFeeForMessage",
        "params": [message_base64],
        "id": 1,
    });
    let json = rpc_post(client, solana_rpc_url()?, &body).await?;
    parse_solana_fee_for_message(&json)
}

pub(crate) fn parse_solana_fee_for_message(json: &serde_json::Value) -> Result<u64, String> {
    json_rpc_result(json, "Solana fee")?["value"]
        .as_u64()
        .ok_or_else(|| "Solana fee RPC missing result.value".to_string())
}

#[cfg(test)]
#[path = "../tests/providers/solana.rs"]
mod tests;
