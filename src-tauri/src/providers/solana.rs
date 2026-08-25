use crate::assets::{cached_asset, cached_asset_by_token_address};
use crate::dto::Asset;
use crate::providers::NetworkAssetRefresh;
use crate::providers::http::rpc_post;
use crate::registry::{NetworkConfig, network_by_id};
use std::collections::BTreeMap;

const SOLANA_TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

fn solana_config() -> Result<&'static NetworkConfig, String> {
    network_by_id("solana").ok_or_else(|| "Solana is missing from the network registry".to_string())
}

fn solana_rpc_url() -> Result<&'static str, String> {
    solana_config()?.rpc_url()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SolanaTokenAccount {
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

pub(crate) async fn fetch_solana_native_balance(address: &str) -> Result<u128, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getBalance",
        "params": [address],
        "id": 1,
    });
    let json = rpc_post(solana_rpc_url()?, &body).await?;
    parse_solana_balance(&json)
}

pub(crate) async fn fetch_solana_token_accounts(
    address: &str,
) -> Result<Vec<SolanaTokenAccount>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getTokenAccountsByOwner",
        "params": [address, {"programId": SOLANA_TOKEN_PROGRAM_ID}, {"encoding": "jsonParsed"}],
        "id": 1,
    });
    let json = rpc_post(solana_rpc_url()?, &body).await?;
    parse_solana_token_accounts(&json, address)
}

pub(crate) async fn fetch_solana_assets(
    address: &str,
    cached_assets: &[Asset],
) -> NetworkAssetRefresh {
    let config = solana_config().expect("Solana must exist in the generated network registry");
    let mut balance_failed = false;
    let mut assets = vec![];

    match fetch_solana_native_balance(address).await {
        Ok(lamports) => assets.push(Asset {
            symbol: config.native_asset.symbol.clone(),
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
            if let Some(cached) =
                cached_asset(cached_assets, &config.id, &config.native_asset.symbol)
            {
                assets.push(cached);
            }
        }
    }

    let token_accounts = match fetch_solana_token_accounts(address).await {
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

    let mut token_balances = BTreeMap::<String, (u128, u8)>::new();
    for account in token_accounts {
        if account.amount == 0 {
            continue;
        }
        let entry = token_balances
            .entry(account.mint)
            .or_insert((0, account.decimals));
        if entry.1 == account.decimals {
            entry.0 += u128::from(account.amount);
        }
    }

    for (mint, (amount, decimals)) in token_balances {
        let cached = cached_asset_by_token_address(cached_assets, "solana", &mint);
        let symbol = cached
            .as_ref()
            .map(|asset| asset.symbol.clone())
            .unwrap_or_else(|| fallback_solana_token_symbol(&mint));
        let name = cached
            .as_ref()
            .map(|asset| asset.name.clone())
            .unwrap_or_else(|| format!("SPL Token {}", short_mint(&mint)));
        let price_usd = cached.as_ref().map(|asset| asset.price_usd).unwrap_or(0.0);
        let change_24h = cached.as_ref().map(|asset| asset.change_24h).unwrap_or(0.0);

        assets.push(Asset {
            symbol,
            name,
            balance: amount.to_string(),
            decimals: u32::from(decimals),
            price_usd,
            change_24h,
            network: "solana".to_string(),
            token_address: Some(mint),
        });
    }
    NetworkAssetRefresh {
        assets,
        balance_failed,
    }
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
    json["result"]["value"]
        .as_u64()
        .map(|value| value as u128)
        .ok_or_else(|| "Solana balance RPC missing result.value".to_string())
}

pub(crate) fn parse_solana_token_accounts(
    json: &serde_json::Value,
    expected_owner: &str,
) -> Result<Vec<SolanaTokenAccount>, String> {
    let accounts = json["result"]["value"]
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
            address: address.to_string(),
            mint: mint.to_string(),
            amount,
            decimals,
        });
    }

    Ok(parsed)
}

pub(crate) async fn broadcast_solana_transaction(raw_tx_base64: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "sendTransaction",
        "params": [raw_tx_base64, {"encoding": "base64"}],
        "id": 1,
    });
    let json = rpc_post(solana_rpc_url()?, &body).await?;
    json["result"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| {
            json["error"]["message"]
                .as_str()
                .unwrap_or("Unknown Solana broadcast error")
                .to_string()
        })
}

pub(crate) async fn fetch_solana_tx_status(signature: &str) -> Result<Option<String>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getSignatureStatuses",
        "params": [[signature], {"searchTransactionHistory": true}],
        "id": 1,
    });
    let json = rpc_post(solana_rpc_url()?, &body).await?;
    parse_solana_tx_status(&json)
}

pub(crate) async fn fetch_solana_token_account_state(
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
    let json = rpc_post(solana_rpc_url()?, &body).await?;
    parse_solana_token_account_state(&json, expected_owner, expected_mint)
}

pub(crate) async fn fetch_solana_mint_decimals(mint: &str) -> Result<u8, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [mint, {"encoding": "jsonParsed"}],
        "id": 1,
    });
    let json = rpc_post(solana_rpc_url()?, &body).await?;
    parse_solana_mint_decimals(&json)
}

pub(crate) fn parse_solana_token_account_state(
    json: &serde_json::Value,
    expected_owner: &str,
    expected_mint: &str,
) -> Result<Option<SolanaTokenAccountState>, String> {
    if let Some(error) = json.get("error") {
        return Err(format!("Solana account info RPC error: {error}"));
    }

    let value = &json["result"]["value"];
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
    if let Some(error) = json.get("error") {
        return Err(format!("Solana mint info RPC error: {error}"));
    }

    let value = &json["result"]["value"];
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

pub(crate) async fn simulate_solana_transaction(raw_tx_base64: &str) -> Result<(), String> {
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
    let json = rpc_post(solana_rpc_url()?, &body).await?;
    parse_solana_simulation(&json)
}

pub(crate) fn parse_solana_simulation(json: &serde_json::Value) -> Result<(), String> {
    if let Some(error) = json.get("error") {
        return Err(format!("Solana simulation RPC error: {error}"));
    }
    let value = json["result"]
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

pub(crate) async fn fetch_solana_token_account_rent() -> Result<u64, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getMinimumBalanceForRentExemption",
        "params": [165],
        "id": 1,
    });
    let json = rpc_post(solana_rpc_url()?, &body).await?;
    parse_solana_rent_exemption(&json)
}

pub(crate) fn parse_solana_rent_exemption(json: &serde_json::Value) -> Result<u64, String> {
    if let Some(error) = json.get("error") {
        return Err(format!("Solana rent exemption RPC error: {error}"));
    }

    json["result"]
        .as_u64()
        .ok_or_else(|| "Solana rent exemption RPC missing result".to_string())
}

pub(crate) async fn fetch_latest_solana_blockhash() -> Result<String, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getLatestBlockhash",
        "params": [{"commitment": "finalized"}],
        "id": 1,
    });
    let json = rpc_post(solana_rpc_url()?, &body).await?;
    parse_latest_solana_blockhash(&json)
}

pub(crate) fn parse_latest_solana_blockhash(json: &serde_json::Value) -> Result<String, String> {
    if let Some(error) = json.get("error") {
        return Err(format!("Solana blockhash RPC error: {error}"));
    }
    json["result"]["value"]["blockhash"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Solana blockhash RPC missing result.value.blockhash".to_string())
}

pub(crate) fn parse_solana_tx_status(json: &serde_json::Value) -> Result<Option<String>, String> {
    if let Some(error) = json.get("error") {
        return Err(format!("Solana status RPC error: {error}"));
    }

    let Some(status) = json["result"]["value"]
        .as_array()
        .and_then(|items| items.first())
    else {
        return Ok(None);
    };
    if status.is_null() {
        return Ok(None);
    }
    if !status["err"].is_null() {
        return Ok(Some("failed".to_string()));
    }
    Ok(Some("confirmed".to_string()))
}

#[allow(dead_code)]
pub(crate) async fn fetch_solana_fee_for_message(message_base64: &str) -> Result<u64, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getFeeForMessage",
        "params": [message_base64],
        "id": 1,
    });
    let json = rpc_post(solana_rpc_url()?, &body).await?;
    parse_solana_fee_for_message(&json)
}

pub(crate) fn parse_solana_fee_for_message(json: &serde_json::Value) -> Result<u64, String> {
    if let Some(error) = json.get("error") {
        return Err(format!("Solana fee RPC error: {error}"));
    }
    json["result"]["value"]
        .as_u64()
        .ok_or_else(|| "Solana fee RPC missing result.value".to_string())
}
