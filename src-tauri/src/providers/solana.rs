use crate::assets::cached_asset;
use crate::dto::Asset;
use crate::providers::http::rpc_post;

const SOLANA_RPC_URL: &str = "https://solana-rpc.publicnode.com";
const SOLANA_TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SolanaTokenAccount {
    pub(crate) mint: String,
    pub(crate) amount: String,
    pub(crate) decimals: u32,
}

pub(crate) async fn fetch_solana_native_balance(address: &str) -> Result<u128, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getBalance",
        "params": [address],
        "id": 1,
    });
    let json = rpc_post(SOLANA_RPC_URL, &body).await?;
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
    let json = rpc_post(SOLANA_RPC_URL, &body).await?;
    parse_solana_token_accounts(&json)
}

pub(crate) async fn fetch_solana_assets(address: &str, cached_assets: &[Asset]) -> Vec<Asset> {
    let native = match fetch_solana_native_balance(address).await {
        Ok(lamports) => Asset {
            symbol: "SOL".to_string(),
            name: "Solana".to_string(),
            balance: lamports.to_string(),
            decimals: 9,
            price_usd: 0.0,
            change_24h: 0.0,
            network: "solana".to_string(),
        },
        Err(_) => cached_asset(cached_assets, "solana", "SOL").unwrap_or_else(|| Asset {
            symbol: "SOL".to_string(),
            name: "Solana".to_string(),
            balance: "0".to_string(),
            decimals: 9,
            price_usd: 0.0,
            change_24h: 0.0,
            network: "solana".to_string(),
        }),
    };

    let mut assets = vec![native];

    let token_accounts = match fetch_solana_token_accounts(address).await {
        Ok(accounts) => accounts,
        Err(_) => {
            assets.extend(cached_solana_token_assets(cached_assets));
            return assets;
        }
    };

    for account in token_accounts {
        if account.amount == "0" {
            continue;
        }
        let (symbol, name) = solana_token_display(cached_assets, &account.mint);

        assets.push(Asset {
            symbol,
            name,
            balance: account.amount,
            decimals: account.decimals,
            price_usd: 0.0,
            change_24h: 0.0,
            network: "solana".to_string(),
        });
    }
    assets
}

fn cached_solana_token_assets(cached_assets: &[Asset]) -> impl Iterator<Item = Asset> + '_ {
    cached_assets
        .iter()
        .filter(|asset| asset.network == "solana" && asset.symbol != "SOL")
        .cloned()
}

fn solana_token_display(cached_assets: &[Asset], mint: &str) -> (String, String) {
    let fallback_symbol = solana_token_symbol(mint);
    let fallback_name = format!("SPL Token {}", short_mint(mint));
    let cached = cached_assets.iter().find(|asset| {
        asset.network == "solana"
            && asset.symbol != "SOL"
            && (asset.name == mint
                || asset.name == fallback_name
                || asset.symbol == fallback_symbol)
    });

    cached
        .map(|asset| (asset.symbol.clone(), asset.name.clone()))
        .unwrap_or((fallback_symbol, fallback_name))
}

fn solana_token_symbol(mint: &str) -> String {
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
) -> Result<Vec<SolanaTokenAccount>, String> {
    let accounts = json["result"]["value"]
        .as_array()
        .ok_or_else(|| "Solana token accounts RPC missing result.value".to_string())?;
    let mut parsed = Vec::new();

    for account in accounts {
        let info = &account["account"]["data"]["parsed"]["info"];
        let Some(mint) = info["mint"].as_str() else {
            continue;
        };
        let token_amount = &info["tokenAmount"];
        let Some(amount) = token_amount["amount"].as_str() else {
            continue;
        };
        let decimals = token_amount["decimals"].as_u64().unwrap_or(0) as u32;
        parsed.push(SolanaTokenAccount {
            mint: mint.to_string(),
            amount: amount.to_string(),
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
    let json = rpc_post(SOLANA_RPC_URL, &body).await?;
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
    let json = rpc_post(SOLANA_RPC_URL, &body).await?;
    parse_solana_tx_status(&json)
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
    let json = rpc_post(SOLANA_RPC_URL, &body).await?;
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
