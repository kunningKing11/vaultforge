use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use argon2::Argon2;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use bech32::{self, ToBase32, Variant};
use bip39::{Language, Mnemonic};
use bs58;
use chrono::Utc;
use ed25519_dalek::{PublicKey as DalekPublicKey, SecretKey as DalekSecretKey};
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::SigningKey;
use rand::Rng;
use ripemd::Ripemd160;
use serde::{Deserialize, Serialize};
use sha2::{Digest as Sha2Digest, Sha256};
use sha3::Keccak256;
use std::{collections::HashMap, fs, path::PathBuf, sync::Mutex};
use tauri::{Manager, State};

#[derive(Clone, Deserialize, Serialize)]
struct Wallet {
    name: String,
    mnemonic: String,
    created_at: String,
    address: String,
    addresses: HashMap<String, String>,
    passphrase_hash: String,
    assets: Vec<Asset>,
    activity: Vec<Activity>,
}

#[derive(Clone, Deserialize, Serialize)]
struct WalletPayload {
    wallet_name: String,
    mnemonic: String,
    created_at: String,
    address: String,
    addresses: HashMap<String, String>,
    passphrase_hash: String,
    assets: Vec<Asset>,
    activity: Vec<Activity>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Asset {
    symbol: String,
    name: String,
    balance: String,
    decimals: u32,
    price_usd: f64,
    change_24h: f64,
    network: String,
}

#[derive(Clone, Deserialize, Serialize)]
struct Activity {
    id: String,
    kind: String,
    title: String,
    subtitle: String,
    amount: String,
    status: String,
    timestamp: String,
    hash: String,
    from: Option<String>,
    to: Option<String>,
    network: Option<String>,
    payload_hash: Option<String>,
    signature: Option<String>,
    fee: Option<String>,
}

#[derive(Serialize)]
struct WalletSession {
    has_wallet: bool,
    locked: bool,
    wallet_name: Option<String>,
    address: Option<String>,
    addresses: Option<HashMap<String, String>>,
    assets: Vec<Asset>,
    activity: Vec<Activity>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SignedTransaction {
    from: String,
    to: String,
    symbol: String,
    amount: String,
    note: String,
    network: String,
    nonce: String,
    signed_at: String,
    payload_hash: String,
    signature: String,
    fee_amount: String,
    fee_symbol: String,
    total_debit: String,
    post_balance: String,
    decimals: u32,
    fiat_value: f64,
    raw_tx: Option<String>,
    tx_hash: Option<String>,
}

#[derive(Clone, Copy)]
struct EvmNetworkConfig {
    id: &'static str,
    display_name: &'static str,
    chain_id: u64,
    native_symbol: &'static str,
    rpc_url: &'static str,
}

const DEFAULT_EVM_CONFIG: &EvmNetworkConfig = &EVM_NETWORKS[0];

const EVM_NETWORKS: &[EvmNetworkConfig] = &[
    EvmNetworkConfig {
        id: "ethereum",
        display_name: "Ethereum",
        chain_id: 1,
        native_symbol: "ETH",
        rpc_url: "https://ethereum-rpc.publicnode.com",
    },
    EvmNetworkConfig {
        id: "monad",
        display_name: "Monad",
        chain_id: 167004,
        native_symbol: "MON",
        rpc_url: "https://rpc.monad.xyz",
    },
    EvmNetworkConfig {
        id: "polygon",
        display_name: "Polygon",
        chain_id: 137,
        native_symbol: "MATIC",
        rpc_url: "https://polygon-bor-rpc.publicnode.com",
    },
    EvmNetworkConfig {
        id: "arbitrum_one",
        display_name: "Arbitrum One",
        chain_id: 42161,
        native_symbol: "ETH",
        rpc_url: "https://arbitrum-one-rpc.publicnode.com",
    },
    EvmNetworkConfig {
        id: "base",
        display_name: "Base",
        chain_id: 8453,
        native_symbol: "ETH",
        rpc_url: "https://base-rpc.publicnode.com",
    },
    EvmNetworkConfig {
        id: "optimism",
        display_name: "Optimism",
        chain_id: 10,
        native_symbol: "ETH",
        rpc_url: "https://optimism-rpc.publicnode.com",
    },
    EvmNetworkConfig {
        id: "avalanche_c",
        display_name: "Avalanche C-Chain",
        chain_id: 43114,
        native_symbol: "AVAX",
        rpc_url: "https://avalanche-c-chain-rpc.publicnode.com",
    },
];

#[derive(Clone, Copy)]
struct EvmTokenConfig {
    symbol: &'static str,
    name: &'static str,
    contract: &'static str,
    decimals: u32,
}

const EVM_TOKENS: &[(&str, &[EvmTokenConfig])] = &[
    ("ethereum", &[
        EvmTokenConfig { symbol: "USDC", name: "USD Coin", contract: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", decimals: 6 },
        EvmTokenConfig { symbol: "USDT", name: "Tether USD", contract: "0xdAC17F958D2ee523a2206206994597C13D831ec7", decimals: 6 },
    ]),
    ("polygon", &[
        EvmTokenConfig { symbol: "USDC", name: "USD Coin", contract: "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174", decimals: 6 },
        EvmTokenConfig { symbol: "USDT", name: "Tether USD", contract: "0xc2132D05D31c914a87C6611C10748AEb04B58e8F", decimals: 6 },
    ]),
    ("arbitrum_one", &[
        EvmTokenConfig { symbol: "USDC", name: "USD Coin", contract: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831", decimals: 6 },
    ]),
    ("base", &[
        EvmTokenConfig { symbol: "USDC", name: "USD Coin", contract: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913", decimals: 6 },
    ]),
    ("optimism", &[
        EvmTokenConfig { symbol: "USDC", name: "USD Coin", contract: "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85", decimals: 6 },
    ]),
    ("avalanche_c", &[
        EvmTokenConfig { symbol: "USDC", name: "USD Coin", contract: "0xB97EF9Ef8734C71904D8002F8b6Bc66Dd9c48a6E", decimals: 6 },
    ]),
];

fn evm_tokens_for_network(network_id: &str) -> &[EvmTokenConfig] {
    EVM_TOKENS.iter()
        .find(|(id, _)| *id == network_id)
        .map(|(_, tokens)| *tokens)
        .unwrap_or(&[])
}

async fn fetch_evm_token_balance(config: &EvmNetworkConfig, token: &EvmTokenConfig, address: &str) -> Result<u128, String> {
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
            "to": token.contract,
            "data": format!("0x{}", hex::encode(&data))
        }, "latest"],
        "id": 1,
    });

    let response = reqwest::Client::new()
        .post(config.rpc_url)
        .json(&body)
        .header("user-agent", "VaultForge Wallet/0.1.0")
        .send()
        .await
        .map_err(|e| format!("Token balance RPC failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Token balance RPC returned HTTP {}", response.status()));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Token balance response parse failed: {e}"))?;

    let hex_str = json["result"]
        .as_str()
        .ok_or_else(|| "Token balance RPC missing result".to_string())?;

    u128::from_str_radix(hex_str.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid token balance hex: {e}"))
}

#[derive(Deserialize)]
struct CoinGeckoPrice {
    usd: f64,
    usd_24h_change: Option<f64>,
}

type CoinGeckoPriceResponse = HashMap<String, CoinGeckoPrice>;

struct AppState {
    wallet: Option<Wallet>,
    locked: bool,
    stored_wallet: Option<StoredWalletMetadata>,
    encryption_key: Option<[u8; 32]>,
    storage_salt: Option<Vec<u8>>,
    storage_path: PathBuf,
}

#[derive(Clone)]
struct StoredWalletMetadata {
    wallet_name: String,
}

#[derive(Deserialize, Serialize)]
struct StoredWalletFile {
    version: u8,
    wallet_name: String,
    network: String,
    salt: String,
    nonce: String,
    ciphertext: String,
}

impl AppState {
    fn from_storage(storage_path: PathBuf) -> Self {
        let stored_wallet = read_stored_wallet(&storage_path)
            .ok()
            .flatten()
            .map(|stored| StoredWalletMetadata {
                wallet_name: stored.wallet_name,
            });
        Self {
            wallet: None,
            locked: stored_wallet.is_some(),
            stored_wallet,
            encryption_key: None,
            storage_salt: None,
            storage_path,
        }
    }
}

#[tauri::command]
fn get_wallet(state: State<'_, Mutex<AppState>>) -> Result<WalletSession, String> {
    let state = state.lock().map_err(|_| "State lock failed")?;
    Ok(session_from_state(&state))
}

#[tauri::command]
async fn refresh_prices(state: State<'_, Mutex<AppState>>) -> Result<WalletSession, String> {
    let price_ids = {
        let state = state.lock().map_err(|_| "State lock failed")?;
        if state.locked {
            return Err("Wallet is locked".to_string());
        }
        let wallet = state
            .wallet
            .as_ref()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        wallet
            .assets
            .iter()
            .filter_map(|asset| price_id_for_symbol(&asset.symbol))
            .fold(Vec::<&'static str>::new(), |mut ids, id| {
                if !ids.contains(&id) {
                    ids.push(id);
                }
                ids
            })
    };

    if price_ids.is_empty() {
        let state = state.lock().map_err(|_| "State lock failed")?;
        return Ok(session_from_state(&state));
    }

    let prices = fetch_market_prices(&price_ids).await?;

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    {
        let wallet = state
            .wallet
            .as_mut()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        for asset in &mut wallet.assets {
            let Some(price_id) = price_id_for_symbol(&asset.symbol) else {
                continue;
            };
            let Some(price) = prices.get(price_id) else {
                continue;
            };
            if price.usd.is_finite() && price.usd > 0.0 {
                asset.price_usd = price.usd;
            }
            if let Some(change) = price.usd_24h_change {
                if change.is_finite() {
                    asset.change_24h = change;
                }
            }
        }
    }
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

#[tauri::command]
async fn create_wallet(
    state: State<'_, Mutex<AppState>>,
    name: String,
    passphrase: String,
) -> Result<WalletSession, String> {
    validate_passphrase(&passphrase)?;
    let mnemonic = generate_mnemonic()?;
    let addresses = derive_addresses_from_mnemonic(&mnemonic)?;
    let primary_address = addresses
        .get("evm")
        .cloned()
        .unwrap_or_else(|| address_from_seed(&mnemonic));

    let assets = fetch_evm_assets(DEFAULT_EVM_CONFIG, &primary_address).await;

    let wallet = Wallet {
        name: clean_name(name),
        mnemonic,
        created_at: Utc::now().to_rfc3339(),
        address: primary_address,
        addresses,
        passphrase_hash: hash_secret(&passphrase),
        assets,
        activity: vec![activity(
            "system",
            "Wallet created",
            "Recovery phrase generated locally",
            "12 words",
        )],
    };

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    let (key, salt) = derive_storage_key(&passphrase, None)?;
    state.encryption_key = Some(key);
    state.storage_salt = Some(salt);
    state.wallet = Some(wallet);
    state.locked = false;
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

#[tauri::command]
async fn import_wallet(
    state: State<'_, Mutex<AppState>>,
    mnemonic: String,
    passphrase: String,
) -> Result<WalletSession, String> {
    let words = mnemonic.split_whitespace().count();
    if words != 12 && words != 24 {
        return Err("Recovery phrase must contain 12 or 24 words".to_string());
    }
    validate_passphrase(&passphrase)?;

    let mnemonic = mnemonic.trim().to_string();
    let addresses = derive_addresses_from_mnemonic(&mnemonic)?;
    let primary_address = addresses
        .get("evm")
        .cloned()
        .unwrap_or_else(|| address_from_seed(&mnemonic));

    let assets = fetch_evm_assets(DEFAULT_EVM_CONFIG, &primary_address).await;

    let wallet = Wallet {
        name: "Imported Wallet".to_string(),
        mnemonic,
        created_at: Utc::now().to_rfc3339(),
        address: primary_address,
        addresses,
        passphrase_hash: hash_secret(&passphrase),
        assets,
        activity: vec![activity(
            "import",
            "Wallet imported",
            "Recovery phrase verified locally",
            "Imported",
        )],
    };

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    let (key, salt) = derive_storage_key(&passphrase, None)?;
    state.encryption_key = Some(key);
    state.storage_salt = Some(salt);
    state.wallet = Some(wallet);
    state.locked = false;
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

#[tauri::command]
async fn unlock_wallet(
    state: State<'_, Mutex<AppState>>,
    passphrase: String,
) -> Result<WalletSession, String> {
    let passphrase_hash = hash_secret(&passphrase);

    let address = {
        let mut state = state.lock().map_err(|_| "State lock failed")?;

        let in_memory = state.wallet.as_ref().map(|w| {
            (w.passphrase_hash.clone(), w.address.clone())
        });

        if let Some((stored_hash, addr)) = in_memory {
            if stored_hash != passphrase_hash {
                return Err("Invalid passphrase".to_string());
            }
            state.locked = false;
            addr
        } else {
            let stored = read_stored_wallet(&state.storage_path)?
                .ok_or_else(|| "No wallet exists yet".to_string())?;
            let wallet = decrypt_wallet(&stored, &passphrase)?;
            if wallet.passphrase_hash != passphrase_hash {
                return Err("Invalid passphrase".to_string());
            }
            state.stored_wallet = Some(StoredWalletMetadata {
                wallet_name: stored.wallet_name,
            });
            let salt = BASE64
                .decode(stored.salt)
                .map_err(|_| "Stored wallet salt is invalid")?;
            let (key, salt) = derive_storage_key(&passphrase, Some(&salt))?;
            state.encryption_key = Some(key);
            state.storage_salt = Some(salt);
            let address = wallet.address.clone();
            state.wallet = Some(wallet);
            state.locked = false;
            address
        }
    };

    let fresh_assets = fetch_evm_assets(DEFAULT_EVM_CONFIG, &address).await;

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if let Some(wallet) = state.wallet.as_mut() {
        wallet.assets = fresh_assets;
    }
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

#[tauri::command]
fn lock_wallet(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if state.wallet.is_some() {
        state.wallet = None;
        state.encryption_key = None;
        state.storage_salt = None;
        state.locked = true;
    }
    Ok(())
}

#[tauri::command]
fn clear_wallet(state: State<'_, Mutex<AppState>>) -> Result<WalletSession, String> {
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if state.storage_path.exists() {
        fs::remove_file(&state.storage_path).map_err(|_| "Failed to remove stored wallet")?;
    }
    state.wallet = None;
    state.stored_wallet = None;
    state.encryption_key = None;
    state.storage_salt = None;
    state.locked = false;
    // network concept removed
    Ok(session_from_state(&state))
}

#[tauri::command]
async fn sign_transaction(
    state: State<'_, Mutex<AppState>>,
    to: String,
    symbol: String,
    amount: String,
    note: String,
) -> Result<SignedTransaction, String> {
    validate_unlocked(&state)?;

    let (mnemonic, address, assets, decimals) = {
        let state = state.lock().map_err(|_| "State lock failed")?;
        let wallet = state
            .wallet
            .as_ref()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        validate_transfer(wallet, &to, &symbol, &amount)?;
        let decimals = wallet
            .assets
            .iter()
            .find(|a| a.symbol == symbol)
            .map(|a| a.decimals)
            .unwrap_or(18);
        (
            wallet.mnemonic.clone(),
            wallet.address.clone(),
            wallet.assets.clone(),
            decimals,
        )
    };

    let to = to.trim().to_string();

    let config = evm_config_for_symbol(&symbol)
        .copied()
        .ok_or_else(|| format!("No EVM chain configured for {}", symbol))?;

    let value_wei: u128 = amount.parse().map_err(|_| "Invalid amount".to_string())?;
    let nonce = fetch_evm_nonce(&config, &address).await?;
    let gas_price = fetch_evm_gas_price(&config).await?;
    let gas_limit = fetch_evm_estimate_gas(&config, &address, &to, value_wei).await?;

    let max_priority_fee_per_gas = gas_price;
    let max_fee_per_gas = gas_price;
    let total_fee_wei = gas_limit as u128 * max_fee_per_gas;
    let total_debit_wei = value_wei + total_fee_wei;

    let signing_key = signing_key_from_mnemonic(&mnemonic)?;
    let (_, tx_hash, raw_tx_hex, r_hex, s_hex) = sign_eip1559_transfer(
        &signing_key,
        config.chain_id,
        nonce,
        max_priority_fee_per_gas,
        max_fee_per_gas,
        gas_limit,
        &to,
        value_wei,
    )?;

    let amount_str = value_wei.to_string();
    let fee_str = total_fee_wei.to_string();
    let total_debit_str = total_debit_wei.to_string();
    let signature_str = format!("0x{}{}", r_hex, s_hex);

    let default_asset = Asset {
        symbol: symbol.clone(),
        name: String::new(),
        balance: "0".to_string(),
        decimals: 18,
        price_usd: 0.0,
        change_24h: 0.0,
        network: config.id.to_string(),
    };
    let asset = assets
        .iter()
        .find(|a| a.symbol == symbol)
        .unwrap_or(&default_asset);

    let post_balance_wei: u128 = asset.balance.parse().unwrap_or(0);
    let post_balance = if post_balance_wei >= total_debit_wei {
        (post_balance_wei - total_debit_wei).to_string()
    } else {
        "0".to_string()
    };

    let signed = SignedTransaction {
        from: address,
        to: to.clone(),
        symbol: symbol.clone(),
        amount: amount_str,
        note: note.trim().to_string(),
        network: config.id.to_string(),
        nonce: nonce.to_string(),
        signed_at: Utc::now().to_rfc3339(),
        payload_hash: tx_hash.clone(),
        signature: signature_str,
        fee_amount: fee_str,
        fee_symbol: symbol.clone(),
        total_debit: total_debit_str,
        post_balance,
        decimals,
        fiat_value: 0.0,
        raw_tx: Some(raw_tx_hex),
        tx_hash: Some(tx_hash),
    };

    Ok(signed)
}

#[tauri::command]
async fn send_transaction(
    state: State<'_, Mutex<AppState>>,
    signed: SignedTransaction,
) -> Result<WalletSession, String> {
    validate_unlocked(&state)?;

    {
        let state = state.lock().map_err(|_| "State lock failed")?;
        let wallet = state
            .wallet
            .as_ref()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        if signed.from != wallet.address {
            return Err("Signed transaction does not match this wallet".to_string());
        }
    }

    let raw_tx = signed
        .raw_tx
        .as_ref()
        .ok_or_else(|| "No raw transaction data".to_string())?;

    let config = evm_config_for_symbol(&signed.symbol)
        .ok_or_else(|| format!("No EVM chain configured for {}", signed.symbol))?;

    let tx_hash = broadcast_evm_transaction(config, raw_tx).await?;

    let memo = if signed.note.is_empty() {
        format!("Sent to {}", short_address(&signed.to))
    } else {
        signed.note.clone()
    };

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if let Some(wallet) = state.wallet.as_mut() {
        wallet.activity.insert(
            0,
            Activity {
                id: random_hex(8),
                kind: "send".to_string(),
                title: "Transfer sent".to_string(),
                subtitle: memo,
                amount: format!("-{} {}", signed.amount, signed.symbol),
                status: "pending".to_string(),
                timestamp: Utc::now().to_rfc3339(),
                hash: tx_hash.clone(),
                from: Some(signed.from.clone()),
                to: Some(signed.to.clone()),
                network: Some(signed.network.clone()),
                payload_hash: signed.tx_hash.clone(),
                signature: Some(signed.signature.clone()),
                fee: Some(format!("{} {}", signed.fee_amount, signed.fee_symbol)),
            },
        );
    }
    persist_state_wallet(&mut state)?;
    let session = session_from_state(&state);
    Ok(session)
}

#[tauri::command(rename_all = "camelCase")]
fn swap_tokens(
    state: State<'_, Mutex<AppState>>,
    from_symbol: String,
    to_symbol: String,
    amount: String,
) -> Result<WalletSession, String> {
    validate_unlocked(&state)?;
    if from_symbol == to_symbol {
        return Err("Choose two different assets".to_string());
    }

    let amount_wei: u128 = amount.parse().map_err(|_| "Invalid amount".to_string())?;
    if amount_wei == 0 {
        return Err("Amount must be greater than zero".to_string());
    }

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    let wallet = state
        .wallet
        .as_mut()
        .ok_or_else(|| "No wallet exists yet".to_string())?;

    let from_index = wallet
        .assets
        .iter()
        .position(|asset| asset.symbol == from_symbol)
        .ok_or_else(|| "Source asset not found".to_string())?;
    let to_index = wallet
        .assets
        .iter()
        .position(|asset| asset.symbol == to_symbol)
        .ok_or_else(|| "Destination asset not found".to_string())?;

    let from_balance: u128 = wallet.assets[from_index].balance.parse().unwrap_or(0);
    if from_balance < amount_wei {
        return Err(format!("Insufficient {} balance", from_symbol));
    }

    let source_value = amount_wei as f64 / 1e18 * wallet.assets[from_index].price_usd;
    let received_wei = if wallet.assets[to_index].price_usd > 0.0 {
        let received_f64 = (source_value / wallet.assets[to_index].price_usd) * 0.995;
        (received_f64 * 1e18) as u128
    } else {
        0
    };

    wallet.assets[from_index].balance = (from_balance - amount_wei).to_string();
    let to_balance: u128 = wallet.assets[to_index].balance.parse().unwrap_or(0);
    wallet.assets[to_index].balance = (to_balance + received_wei).to_string();
    wallet.activity.insert(
        0,
        activity(
            "swap",
            "Swap executed",
            &format!("{} to {} with 0.5% route fee", from_symbol, to_symbol),
            &format!("{amount_wei} {from_symbol} -> {received_wei} {to_symbol}"),
        ),
    );
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

fn validate_unlocked(state: &State<'_, Mutex<AppState>>) -> Result<(), String> {
    let state = state.lock().map_err(|_| "State lock failed")?;
    if state.wallet.is_none() {
        return Err("No wallet exists yet".to_string());
    }
    if state.locked {
        return Err("Wallet is locked".to_string());
    }
    Ok(())
}

fn validate_transfer(wallet: &Wallet, to: &str, symbol: &str, amount_wei: &str) -> Result<(), String> {
    let to = to.trim();

    let asset = wallet
        .assets
        .iter()
        .find(|asset| asset.symbol == symbol)
        .ok_or_else(|| "Asset not found".to_string())?;

    let amount: u128 = amount_wei.parse().map_err(|_| "Invalid amount".to_string())?;
    let balance: u128 = asset.balance.parse().map_err(|_| "Invalid stored balance".to_string())?;
    if amount == 0 {
        return Err("Amount must be greater than zero".to_string());
    }
    if balance < amount {
        return Err(format!("Insufficient {} balance", symbol));
    }
    validate_address_for_symbol(to, symbol)?;

    Ok(())
}

fn validate_address_for_symbol(address: &str, symbol: &str) -> Result<(), String> {
    match symbol {
        "BTC" => {
            let valid_prefix =
                address.starts_with("bc1") || address.starts_with('1') || address.starts_with('3');
            if valid_prefix && address.len() >= 26 && address.len() <= 62 {
                Ok(())
            } else {
                Err("Recipient must be a valid Bitcoin address".to_string())
            }
        }
        "SOL" => {
            if !address.starts_with("0x") && address.len() >= 32 && address.len() <= 44 {
                Ok(())
            } else {
                Err("Recipient must be a valid Solana address".to_string())
            }
        }
        _ => {
            if address.starts_with("0x") && address.len() >= 12 {
                Ok(())
            } else {
                Err("Recipient must be a valid 0x address".to_string())
            }
        }
    }
}

fn session_from_state(state: &AppState) -> WalletSession {
    let Some(wallet) = state.wallet.as_ref() else {
        if let Some(stored_wallet) = state.stored_wallet.as_ref() {
            return WalletSession {
                has_wallet: true,
                locked: true,
                wallet_name: Some(stored_wallet.wallet_name.clone()),
                address: None,
                addresses: None,
                assets: vec![],
                activity: vec![],
            };
        }

        return WalletSession {
            has_wallet: false,
            locked: false,
            wallet_name: None,
            address: None,
            addresses: None,
            assets: vec![],
            activity: vec![],
        };
    };

    if state.locked {
        return WalletSession {
            has_wallet: true,
            locked: true,
            wallet_name: Some(wallet.name.clone()),
            address: None,
            addresses: None,
            assets: vec![],
            activity: vec![],
        };
    }

    WalletSession {
        has_wallet: true,
        locked: false,
        wallet_name: Some(wallet.name.clone()),
        address: Some(wallet.address.clone()),
        addresses: Some(wallet.addresses.clone()),
        assets: wallet.assets.clone(),
        activity: wallet.activity.clone(),
    }
}

fn evm_config_for_symbol(symbol: &str) -> Option<&'static EvmNetworkConfig> {
    EVM_NETWORKS.iter().find(|c| c.native_symbol == symbol)
}

fn price_id_for_symbol(symbol: &str) -> Option<&'static str> {
    match symbol {
        "ETH" => Some("ethereum"),
        "BTC" => Some("bitcoin"),
        "SOL" => Some("solana"),
        "USDC" => Some("usd-coin"),
        _ => None,
    }
}

async fn fetch_market_prices(ids: &[&str]) -> Result<CoinGeckoPriceResponse, String> {
    let ids = ids.join(",");
    let url = format!(
        "https://api.coingecko.com/api/v3/simple/price?ids={ids}&vs_currencies=usd&include_24hr_change=true"
    );
    let response = reqwest::Client::new()
        .get(url)
        .header("accept", "application/json")
        .header("user-agent", "VaultForge Wallet/0.1.0")
        .send()
        .await
        .map_err(|_| "Failed to reach price service")?;

    if !response.status().is_success() {
        return Err(format!("Price service returned HTTP {}", response.status()));
    }

    response
        .json::<CoinGeckoPriceResponse>()
        .await
        .map_err(|_| "Price service returned invalid data".to_string())
}

async fn fetch_evm_native_balance(config: &EvmNetworkConfig, address: &str) -> Result<u128, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getBalance",
        "params": [address, "latest"],
        "id": 1,
    });

    let response = reqwest::Client::new()
        .post(config.rpc_url)
        .json(&body)
        .header("user-agent", "VaultForge Wallet/0.1.0")
        .send()
        .await
        .map_err(|e| format!("RPC request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("RPC returned HTTP {}", response.status()));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("RPC response parse failed: {e}"))?;

    let balance_hex = json["result"]
        .as_str()
        .ok_or_else(|| "RPC response missing result field".to_string())?;

    u128::from_str_radix(balance_hex.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid balance hex: {e}"))
}

async fn fetch_evm_assets(config: &EvmNetworkConfig, address: &str) -> Vec<Asset> {
    let native = match fetch_evm_native_balance(config, address).await {
        Ok(wei) => Asset {
            symbol: config.native_symbol.to_string(),
            name: config.display_name.to_string(),
            balance: wei.to_string(),
            decimals: 18,
            price_usd: 0.0,
            change_24h: 0.0,
            network: config.id.to_string(),
        },
        Err(_) => Asset {
            symbol: config.native_symbol.to_string(),
            name: config.display_name.to_string(),
            balance: "0".to_string(),
            decimals: 18,
            price_usd: 0.0,
            change_24h: 0.0,
            network: config.id.to_string(),
        },
    };

    let mut assets = vec![native];

    for token in evm_tokens_for_network(config.id) {
        match fetch_evm_token_balance(config, token, address).await {
            Ok(balance) => {
                assets.push(Asset {
                    symbol: token.symbol.to_string(),
                    name: token.name.to_string(),
                    balance: balance.to_string(),
                    decimals: token.decimals,
                    price_usd: 0.0,
                    change_24h: 0.0,
                    network: config.id.to_string(),
                });
            }
            Err(_) => {}
        }
    }

    assets
}

fn u128_to_be_bytes(value: u128) -> Vec<u8> {
    let be = value.to_be_bytes();
    let start = be.iter().position(|&b| b != 0).unwrap_or(be.len() - 1);
    be[start..].to_vec()
}

async fn fetch_evm_nonce(config: &EvmNetworkConfig, address: &str) -> Result<u64, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionCount",
        "params": [address, "latest"],
        "id": 1,
    });
    let response = reqwest::Client::new()
        .post(config.rpc_url)
        .json(&body)
        .header("user-agent", "VaultForge Wallet/0.1.0")
        .send()
        .await
        .map_err(|e| format!("Nonce RPC failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Nonce RPC returned HTTP {}", response.status()));
    }
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Nonce response parse failed: {e}"))?;
    let hex_str = json["result"]
        .as_str()
        .ok_or_else(|| "Nonce RPC missing result".to_string())?;
    u64::from_str_radix(hex_str.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid nonce hex: {e}"))
}

async fn fetch_evm_gas_price(config: &EvmNetworkConfig) -> Result<u128, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_gasPrice",
        "params": [],
        "id": 1,
    });
    let response = reqwest::Client::new()
        .post(config.rpc_url)
        .json(&body)
        .header("user-agent", "VaultForge Wallet/0.1.0")
        .send()
        .await
        .map_err(|e| format!("Gas price RPC failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Gas price RPC returned HTTP {}", response.status()));
    }
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Gas price response parse failed: {e}"))?;
    let hex_str = json["result"]
        .as_str()
        .ok_or_else(|| "Gas price RPC missing result".to_string())?;
    u128::from_str_radix(hex_str.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid gas price hex: {e}"))
}

async fn fetch_evm_estimate_gas(
    config: &EvmNetworkConfig,
    from: &str,
    to: &str,
    value: u128,
) -> Result<u64, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_estimateGas",
        "params": [{
            "from": from,
            "to": to,
            "value": format!("0x{:x}", value),
        }],
        "id": 1,
    });
    let response = reqwest::Client::new()
        .post(config.rpc_url)
        .json(&body)
        .header("user-agent", "VaultForge Wallet/0.1.0")
        .send()
        .await
        .map_err(|e| format!("Estimate gas RPC failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Estimate gas RPC returned HTTP {}",
            response.status()
        ));
    }
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Estimate gas response parse failed: {e}"))?;
    let hex_str = json["result"]
        .as_str()
        .ok_or_else(|| "Estimate gas RPC missing result".to_string())?;
    u64::from_str_radix(hex_str.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid gas estimate hex: {e}"))
}

async fn broadcast_evm_transaction(
    config: &EvmNetworkConfig,
    raw_tx_hex: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_sendRawTransaction",
        "params": [raw_tx_hex],
        "id": 1,
    });
    let response = reqwest::Client::new()
        .post(config.rpc_url)
        .json(&body)
        .header("user-agent", "VaultForge Wallet/0.1.0")
        .send()
        .await
        .map_err(|e| format!("Broadcast RPC failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Broadcast RPC returned HTTP {}",
            response.status()
        ));
    }
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Broadcast response parse failed: {e}"))?;
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

async fn fetch_tx_status(config: &EvmNetworkConfig, tx_hash: &str) -> Result<Option<String>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash],
        "id": 1,
    });
    let response = reqwest::Client::new()
        .post(config.rpc_url)
        .json(&body)
        .header("user-agent", "VaultForge Wallet/0.1.0")
        .send()
        .await
        .map_err(|e| format!("Tx status RPC failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Tx status RPC returned HTTP {}", response.status()));
    }
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Tx status response parse failed: {e}"))?;

    if json["result"].is_null() {
        return Ok(None);
    }

    let status_hex = json["result"]["status"]
        .as_str()
        .unwrap_or("0x0");
    if status_hex == "0x1" {
        Ok(Some("confirmed".to_string()))
    } else {
        Ok(Some("failed".to_string()))
    }
}

#[tauri::command]
async fn check_transaction_status(
    _state: State<'_, Mutex<AppState>>,
    tx_hash: String,
    network: String,
) -> Result<Option<String>, String> {
    let config = EVM_NETWORKS.iter().find(|c| c.id == network)
        .ok_or_else(|| format!("Unknown network: {}", network))?;
    fetch_tx_status(config, &tx_hash).await
}

fn signing_key_from_mnemonic(mnemonic: &str) -> Result<k256::ecdsa::SigningKey, String> {
    let parsed = Mnemonic::parse_in_normalized(Language::English, mnemonic)
        .map_err(|_| "Invalid mnemonic".to_string())?;
    let seed = parsed.to_seed("");
    let seed_bytes = seed.as_ref();
    let private_key: [u8; 32] = Sha256::digest(seed_bytes).into();
    k256::ecdsa::SigningKey::from_bytes((&private_key).into())
        .map_err(|_| "Failed to create signing key".to_string())
}

fn sign_eip1559_transfer(
    private_key: &k256::ecdsa::SigningKey,
    chain_id: u64,
    nonce: u64,
    max_priority_fee_per_gas: u128,
    max_fee_per_gas: u128,
    gas_limit: u64,
    to: &str,
    value: u128,
) -> Result<(Vec<u8>, String, String, String, String), String> {
    let to_bytes = hex::decode(to.trim_start_matches("0x"))
        .map_err(|_| "Invalid to address".to_string())?;
    let empty: Vec<u8> = vec![];

    let max_priority_bytes = u128_to_be_bytes(max_priority_fee_per_gas);
    let max_fee_bytes = u128_to_be_bytes(max_fee_per_gas);
    let value_bytes = u128_to_be_bytes(value);

    let mut stream = rlp::RlpStream::new();
    stream.begin_list(9);
    stream.append(&chain_id);
    stream.append(&nonce);
    stream.append(&max_priority_bytes);
    stream.append(&max_fee_bytes);
    stream.append(&gas_limit);
    stream.append(&to_bytes);
    stream.append(&value_bytes);
    stream.append(&empty);
    stream.begin_list(0);

    let unsigned_data = stream.out().to_vec();

    let mut sig_hash_input = vec![0x02u8];
    sig_hash_input.extend_from_slice(&unsigned_data);
    let sig_hash = Keccak256::digest(&sig_hash_input);

    let signature: k256::ecdsa::Signature = private_key
        .sign_prehash(&sig_hash)
        .map_err(|_| "Transaction signing failed".to_string())?;

    let sig_bytes = signature.to_bytes();
    let r_bytes = &sig_bytes[..32];
    let s_bytes = &sig_bytes[32..];
    let r_vec: Vec<u8> = r_bytes.to_vec();
    let s_vec: Vec<u8> = s_bytes.to_vec();

    let mut y_parity: u64 = 0;
    let verifying_key = private_key.verifying_key();
    for is_odd in [false, true] {
        let rid = k256::ecdsa::RecoveryId::new(is_odd, false);
        if let Ok(recovered) =
            k256::ecdsa::VerifyingKey::recover_from_prehash(&sig_hash, &signature, rid)
        {
            if &recovered == verifying_key {
                y_parity = if is_odd { 1 } else { 0 };
                break;
            }
        }
    }

    let mut tx_stream = rlp::RlpStream::new();
    tx_stream.begin_list(12);
    tx_stream.append(&chain_id);
    tx_stream.append(&nonce);
    tx_stream.append(&max_priority_bytes);
    tx_stream.append(&max_fee_bytes);
    tx_stream.append(&gas_limit);
    tx_stream.append(&to_bytes);
    tx_stream.append(&value_bytes);
    tx_stream.append(&empty);
    tx_stream.begin_list(0);
    tx_stream.append(&y_parity);
    tx_stream.append(&r_vec);
    tx_stream.append(&s_vec);

    let mut signed_data = vec![0x02u8];
    signed_data.extend_from_slice(&tx_stream.out());

    let tx_hash = format!("0x{}", hex::encode(Keccak256::digest(&signed_data)));
    let raw_tx_hex = format!("0x{}", hex::encode(&signed_data));
    let r_hex = hex::encode(r_bytes);
    let s_hex = hex::encode(s_bytes);

    Ok((signed_data, tx_hash, raw_tx_hex, r_hex, s_hex))
}

fn activity(kind: &str, title: &str, subtitle: &str, amount: &str) -> Activity {
    Activity {
        id: random_hex(8),
        kind: kind.to_string(),
        title: title.to_string(),
        subtitle: subtitle.to_string(),
        amount: amount.to_string(),
        status: "confirmed".to_string(),
        timestamp: Utc::now().to_rfc3339(),
        hash: format!("0x{}", random_hex(32)),
        from: None,
        to: None,
        network: None,
        payload_hash: None,
        signature: None,
        fee: None,
    }
}

fn read_stored_wallet(path: &PathBuf) -> Result<Option<StoredWalletFile>, String> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path).map_err(|_| "Failed to read stored wallet")?;
    let stored = serde_json::from_str(&contents).map_err(|_| "Stored wallet file is invalid")?;
    Ok(Some(stored))
}

fn persist_state_wallet(state: &mut AppState) -> Result<(), String> {
    let Some(wallet) = state.wallet.as_ref() else {
        return Ok(());
    };
    let key = state
        .encryption_key
        .ok_or_else(|| "Wallet encryption key is not available".to_string())?;
    let salt = state
        .storage_salt
        .clone()
        .ok_or_else(|| "Wallet encryption salt is not available".to_string())?;

    let stored = encrypt_wallet(wallet, &key, &salt)?;
    if let Some(parent) = state.storage_path.parent() {
        fs::create_dir_all(parent).map_err(|_| "Failed to create wallet storage directory")?;
    }
    let contents = serde_json::to_string_pretty(&stored).map_err(|_| "Failed to encode wallet")?;
    fs::write(&state.storage_path, contents).map_err(|_| "Failed to save wallet")?;
    state.stored_wallet = Some(StoredWalletMetadata {
        wallet_name: stored.wallet_name,
    });
    Ok(())
}

fn encrypt_wallet(
    wallet: &Wallet,
    key: &[u8; 32],
    salt: &[u8],
) -> Result<StoredWalletFile, String> {
    let nonce_bytes: Vec<u8> = (0..12).map(|_| rand::thread_rng().r#gen()).collect();
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "Failed to initialize encryption")?;
    let payload = WalletPayload {
        wallet_name: wallet.name.clone(),
        mnemonic: wallet.mnemonic.clone(),
        created_at: wallet.created_at.clone(),
        address: wallet.address.clone(),
        addresses: wallet.addresses.clone(),
        passphrase_hash: wallet.passphrase_hash.clone(),
        assets: wallet.assets.clone(),
        activity: wallet.activity.clone(),
    };
    let plaintext = serde_json::to_vec(&payload).map_err(|_| "Failed to encode wallet")?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
        .map_err(|_| "Failed to encrypt wallet")?;

    Ok(StoredWalletFile {
        version: 2,
        wallet_name: wallet.name.clone(),
        network: "ethereum".to_string(),
        salt: BASE64.encode(salt),
        nonce: BASE64.encode(nonce_bytes),
        ciphertext: BASE64.encode(ciphertext),
    })
}

fn decrypt_wallet(stored: &StoredWalletFile, passphrase: &str) -> Result<Wallet, String> {
    if stored.version != 2 {
        return Err("Unsupported wallet version".to_string());
    }
    let salt = BASE64
        .decode(&stored.salt)
        .map_err(|_| "Stored wallet salt is invalid")?;
    let nonce = BASE64
        .decode(&stored.nonce)
        .map_err(|_| "Stored wallet nonce is invalid")?;
    let ciphertext = BASE64
        .decode(&stored.ciphertext)
        .map_err(|_| "Stored wallet payload is invalid")?;
    let (key, _) = derive_storage_key(passphrase, Some(&salt))?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "Failed to initialize encryption")?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| "Invalid passphrase")?;
    let payload: WalletPayload = serde_json::from_slice(&plaintext)
        .map_err(|_| "Stored wallet contents are invalid".to_string())?;
    Ok(Wallet {
        name: payload.wallet_name,
        mnemonic: payload.mnemonic,
        created_at: payload.created_at,
        address: payload.address,
        addresses: payload.addresses,
        passphrase_hash: payload.passphrase_hash,
        assets: payload.assets,
        activity: payload.activity,
    })
}

fn derive_storage_key(
    passphrase: &str,
    salt: Option<&[u8]>,
) -> Result<([u8; 32], Vec<u8>), String> {
    let salt = salt
        .map(|value| value.to_vec())
        .unwrap_or_else(|| (0..16).map(|_| rand::thread_rng().r#gen()).collect());
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), &salt, &mut key)
        .map_err(|_| "Failed to derive wallet encryption key")?;
    Ok((key, salt))
}

fn generate_mnemonic() -> Result<String, String> {
    let mut entropy = [0u8; 16];
    let mut rng = rand::thread_rng();
    rng.fill(&mut entropy);
    Mnemonic::from_entropy_in(Language::English, &entropy)
        .map(|mnemonic| mnemonic.to_string())
        .map_err(|_| "Failed to generate recovery phrase".to_string())
}

fn validate_passphrase(passphrase: &str) -> Result<(), String> {
    if passphrase.chars().count() < 8 {
        return Err("Passphrase must be at least 8 characters".to_string());
    }
    Ok(())
}

fn clean_name(name: String) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "Primary Wallet".to_string()
    } else {
        trimmed.chars().take(48).collect()
    }
}

fn address_from_seed(seed: &str) -> String {
    format!("0x{}", &hash_secret(seed)[..40])
}

fn derive_addresses_from_mnemonic(mnemonic: &str) -> Result<HashMap<String, String>, String> {
    let mnemonic = Mnemonic::parse_in_normalized(Language::English, mnemonic)
        .map_err(|_| "Invalid recovery phrase".to_string())?;
    let seed = mnemonic.to_seed("");
    let seed_bytes = seed.as_ref();
    let private_key: [u8; 32] = Sha256::digest(seed_bytes).into();

    let evm_address = ethereum_address_from_private_key(&private_key)?;
    let bitcoin_address = bitcoin_bech32_address(&private_key, false)?;
    let zcash_address = zcash_transparent_address(&private_key, false)?;
    let solana_address = solana_address_from_seed(seed_bytes)?;
    let filecoin_address = filecoin_address_from_private_key(&private_key)?;
    let injective_address = bech32_account_address(&private_key, "inj")?;

    let mut addresses = HashMap::new();
    addresses.insert("evm".to_string(), evm_address);
    addresses.insert("bitcoin".to_string(), bitcoin_address);
    addresses.insert("zcash".to_string(), zcash_address);
    addresses.insert("solana".to_string(), solana_address);
    addresses.insert("filecoin".to_string(), filecoin_address);
    addresses.insert("injective".to_string(), injective_address);
    Ok(addresses)
}

fn signing_key_from_private_key(private_key: &[u8; 32]) -> Result<SigningKey, String> {
    SigningKey::from_bytes(private_key.into()).map_err(|_| "Invalid private key".to_string())
}

fn ethereum_address_from_private_key(private_key: &[u8; 32]) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let public_key = verifying_key.to_encoded_point(false);
    let public_bytes = public_key.as_bytes();
    let hash = Keccak256::digest(&public_bytes[1..]);
    Ok(format!("0x{}", hex::encode(&hash[12..])))
}

fn bitcoin_bech32_address(private_key: &[u8; 32], is_testnet: bool) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let encoded = verifying_key.to_encoded_point(true);
    let public_bytes = encoded.as_bytes();
    let hashed = Ripemd160::digest(&Sha256::digest(public_bytes));
    let hrp = if is_testnet { "tb" } else { "bc" };
    let mut bech32_data = vec![bech32::u5::try_from_u8(0).map_err(|_| "Failed to encode address")?];
    bech32_data.extend(hashed.to_base32());
    bech32::encode(hrp, bech32_data, Variant::Bech32)
        .map_err(|_| "Failed to encode address".to_string())
}

fn zcash_transparent_address(private_key: &[u8; 32], is_testnet: bool) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let encoded = verifying_key.to_encoded_point(true);
    let public_bytes = encoded.as_bytes();
    let payload = Ripemd160::digest(&Sha256::digest(public_bytes));
    let prefix = if is_testnet {
        vec![0x1d, 0x25]
    } else {
        vec![0x1c, 0xb8]
    };
    let mut bytes = prefix;
    bytes.extend(payload);
    Ok(bs58::encode(bytes).with_check().into_string())
}

fn solana_address_from_seed(seed_bytes: &[u8]) -> Result<String, String> {
    let secret_bytes = Sha256::digest(seed_bytes);
    let secret = DalekSecretKey::from_bytes(&secret_bytes)
        .map_err(|_| "Failed to derive Solana key".to_string())?;
    let public = DalekPublicKey::from(&secret);
    Ok(bs58::encode(public.as_bytes()).into_string())
}

fn filecoin_address_from_private_key(private_key: &[u8; 32]) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let encoded = verifying_key.to_encoded_point(true);
    let public_bytes = encoded.as_bytes();
    let payload = Ripemd160::digest(&Sha256::digest(public_bytes));
    let mut bytes = vec![0x01];
    bytes.extend(payload);
    Ok(format!("f{}", bs58::encode(bytes).into_string()))
}

fn bech32_account_address(private_key: &[u8; 32], hrp: &str) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let encoded = verifying_key.to_encoded_point(true);
    let public_bytes = encoded.as_bytes();
    let payload = Ripemd160::digest(&Sha256::digest(public_bytes));
    let bech32_data = payload.to_base32();
    bech32::encode(hrp, bech32_data, Variant::Bech32)
        .map_err(|_| "Failed to encode address".to_string())
}

fn hash_secret(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

fn random_hex(bytes: usize) -> String {
    let mut rng = rand::thread_rng();
    let data: Vec<u8> = (0..bytes).map(|_| rng.r#gen()).collect();
    hex::encode(data)
}

fn short_address(address: &str) -> String {
    if address.len() <= 14 {
        return address.to_string();
    }
    format!("{}...{}", &address[..8], &address[address.len() - 6..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn starter_assets(network: &str) -> Vec<Asset> {
        vec![
            Asset {
                symbol: "ETH".to_string(),
                name: "Ethereum".to_string(),
                balance: "2482100000000000000000".to_string(),
                decimals: 18,
                price_usd: 3480.62,
                change_24h: 2.84,
                network: network.to_string(),
            },
            Asset {
                symbol: "BTC".to_string(),
                name: "Bitcoin".to_string(),
                balance: "184200000000".to_string(),
                decimals: 8,
                price_usd: 102_240.12,
                change_24h: -0.62,
                network: network.to_string(),
            },
            Asset {
                symbol: "SOL".to_string(),
                name: "Solana".to_string(),
                balance: "82450000000".to_string(),
                decimals: 9,
                price_usd: 184.33,
                change_24h: 5.18,
                network: network.to_string(),
            },
            Asset {
                symbol: "USDC".to_string(),
                name: "USD Coin".to_string(),
                balance: "8420000000".to_string(),
                decimals: 6,
                price_usd: 1.0,
                change_24h: 0.01,
                network: network.to_string(),
            },
        ]
    }

    #[test]
    fn validates_asset_address_formats() {
        assert!(validate_address_for_symbol("0x123456789abc", "ETH").is_ok());
        assert!(
            validate_address_for_symbol("bc1q123456789012345678901234567890123456", "BTC").is_ok()
        );
        assert!(validate_address_for_symbol("7".repeat(44).as_str(), "SOL").is_ok());
        assert!(validate_address_for_symbol("0x123456789abc", "BTC").is_err());
        assert!(validate_address_for_symbol("0x123456789abc", "SOL").is_err());
    }

    #[test]
    fn constructs_valid_eip1559_signature() {
        use k256::ecdsa::SigningKey;
        let private_key = [0xabu8; 32];
        let signing_key = SigningKey::from_bytes((&private_key).into()).unwrap();
        let result = sign_eip1559_transfer(
            &signing_key,
            1,
            0,
            1_000_000_000,
            1_000_000_000,
            21000,
            "0xdAC17F958D2ee523a2206206994597C13D831ec7",
            1_000_000_000_000_000_000u128,
        );
        assert!(result.is_ok());
        let (_raw, tx_hash, _raw_hex, r, s) = result.unwrap();
        assert!(tx_hash.starts_with("0x"));
        assert_eq!(tx_hash.len(), 66);
        assert_eq!(r.len(), 64);
        assert_eq!(s.len(), 64);
        assert!(!_raw.is_empty());
    }

    #[test]
    fn derives_same_key_with_same_salt() {
        let (key, salt) = derive_storage_key("correct horse battery staple", None).unwrap();
        let (same_key, same_salt) =
            derive_storage_key("correct horse battery staple", Some(&salt)).unwrap();
        assert_eq!(key, same_key);
        assert_eq!(salt, same_salt);
    }

    #[test]
    fn encrypts_and_decrypts_wallet_payload() {
        let passphrase = "Correct horse battery staple 42!";
        let wallet = Wallet {
            name: "Test Wallet".to_string(),
            mnemonic: "test mnemonic".to_string(),
            created_at: Utc::now().to_rfc3339(),
            address: address_from_seed("test seed"),
            addresses: HashMap::new(),
            passphrase_hash: hash_secret(passphrase),
            assets: starter_assets("ethereum"),
            activity: vec![activity("system", "Created", "Local", "1")],
        };
        let (key, salt) = derive_storage_key(passphrase, None).unwrap();
        let stored = encrypt_wallet(&wallet, &key, &salt).unwrap();
        assert!(!stored.ciphertext.contains(&wallet.address));

        let decrypted = decrypt_wallet(&stored, passphrase).unwrap();
        assert_eq!(decrypted.name, wallet.name);
        assert_eq!(decrypted.mnemonic, wallet.mnemonic);
        assert_eq!(decrypted.created_at, wallet.created_at);
    }

    #[test]
    fn looks_up_evm_network_configs() {
        let ethereum = EVM_NETWORKS.iter().find(|c| c.id == "ethereum").unwrap();
        assert_eq!(ethereum.display_name, "Ethereum");
        assert_eq!(ethereum.chain_id, 1);
        assert_eq!(ethereum.native_symbol, "ETH");
        assert_eq!(ethereum.rpc_url, "https://ethereum-rpc.publicnode.com");

        let avalanche = EVM_NETWORKS.iter().find(|c| c.id == "avalanche_c").unwrap();
        assert_eq!(avalanche.chain_id, 43114);
        assert_eq!(avalanche.native_symbol, "AVAX");
    }

}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let storage_path = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("failed to resolve app data directory: {error}"))?
                .join("wallet.json");
            app.manage(Mutex::new(AppState::from_storage(storage_path)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_wallet,
            refresh_prices,
            create_wallet,
            import_wallet,
            unlock_wallet,
            lock_wallet,
            clear_wallet,
            sign_transaction,
            send_transaction,
            swap_tokens,
            check_transaction_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VaultForge Wallet");
}
