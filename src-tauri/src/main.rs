use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use argon2::Argon2;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use bech32::{self, FromBase32, ToBase32, Variant};
use zeroize::Zeroize;
use bip32::{DerivationPath, XPrv};
use bip39::{Language, Mnemonic};
use bs58;
use chrono::Utc;
use ed25519_dalek::{PublicKey as DalekPublicKey, SecretKey as DalekSecretKey};
use hmac::{Hmac, Mac};
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::SigningKey;
use rand::Rng;
use ripemd::Ripemd160;
use serde::{Deserialize, Serialize};
use sha2::{Digest as Sha2Digest, Sha256, Sha512};
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

#[derive(Clone, Copy)]
struct NativeAssetConfig {
    network_id: &'static str,
    address_key: &'static str,
    symbol: &'static str,
    name: &'static str,
    decimals: u32,
}

#[derive(Clone, Debug)]
struct BitcoinUtxo {
    txid: String,
    vout: u32,
    value: u64,
    confirmed: bool,
}

#[derive(Clone)]
struct BitcoinTxInput {
    utxo: BitcoinUtxo,
    script_code: Vec<u8>,
}

#[derive(Clone)]
struct BitcoinTxOutput {
    value: u64,
    script_pubkey: Vec<u8>,
}

struct BitcoinSignedTransfer {
    txid: String,
    raw_tx_hex: String,
    first_signature_hex: String,
    fee_sats: u64,
    post_balance: u64,
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

const NON_EVM_NATIVE_ASSETS: &[NativeAssetConfig] = &[
    NativeAssetConfig { network_id: "bitcoin", address_key: "bitcoin", symbol: "BTC", name: "Bitcoin", decimals: 8 },
    NativeAssetConfig { network_id: "solana", address_key: "solana", symbol: "SOL", name: "Solana", decimals: 9 },
    NativeAssetConfig { network_id: "zcash", address_key: "zcash", symbol: "ZEC", name: "Zcash", decimals: 8 },
    NativeAssetConfig { network_id: "filecoin", address_key: "filecoin", symbol: "FIL", name: "Filecoin", decimals: 18 },
    NativeAssetConfig { network_id: "injective", address_key: "injective", symbol: "INJ", name: "Injective", decimals: 18 },
];

fn evm_tokens_for_network(network_id: &str) -> &[EvmTokenConfig] {
    EVM_TOKENS.iter()
        .find(|(id, _)| *id == network_id)
        .map(|(_, tokens)| *tokens)
        .unwrap_or(&[])
}

async fn rpc_post(url: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let mut last_err = String::new();
    for attempt in 1..=3 {
        let response = match client
            .post(url)
            .json(body)
            .header("user-agent", "VaultForge Wallet/0.1.0")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("RPC request failed (attempt {attempt}/3): {e}");
                if attempt < 3 {
                    tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
                }
                continue;
            }
        };

        if !response.status().is_success() {
            last_err = format!("RPC returned HTTP {} (attempt {attempt}/3)", response.status());
            if attempt < 3 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
            }
            continue;
        }

        return response.json().await.map_err(|e| format!("RPC response parse failed: {e}"));
    }
    Err(last_err)
}

async fn http_get_json(url: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let mut last_err = String::new();
    for attempt in 1..=3 {
        let response = match client
            .get(url)
            .header("accept", "application/json")
            .header("user-agent", "VaultForge Wallet/0.1.0")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("HTTP request failed (attempt {attempt}/3): {e}");
                if attempt < 3 {
                    tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
                }
                continue;
            }
        };

        if !response.status().is_success() {
            last_err = format!("HTTP returned {} (attempt {attempt}/3)", response.status());
            if attempt < 3 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
            }
            continue;
        }

        return response.json().await.map_err(|e| format!("HTTP response parse failed: {e}"));
    }
    Err(last_err)
}

async fn http_post_text(url: &str, body: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let mut last_err = String::new();
    for attempt in 1..=3 {
        let response = match client
            .post(url)
            .body(body.to_string())
            .header("content-type", "text/plain")
            .header("user-agent", "VaultForge Wallet/0.1.0")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("HTTP POST failed (attempt {attempt}/3): {e}");
                if attempt < 3 {
                    tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
                }
                continue;
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            last_err = format!("HTTP POST returned {status} (attempt {attempt}/3): {text}");
            if attempt < 3 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
            }
            continue;
        }

        return response.text().await.map_err(|e| format!("HTTP response parse failed: {e}"));
    }
    Err(last_err)
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

    let json = rpc_post(config.rpc_url, &body).await?;
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

const EVM_DERIVATION_PATH: &str = "m/44'/60'/0'/0/0";
const BITCOIN_DERIVATION_PATH: &str = "m/84'/0'/0'/0/0";
const ZCASH_DERIVATION_PATH: &str = "m/44'/133'/0'/0/0";
const SOLANA_DERIVATION_PATH: &[u32] = &[44, 501, 0, 0];
const FILECOIN_DERIVATION_PATH: &str = "m/44'/461'/0'/0/0";
const INJECTIVE_DERIVATION_PATH: &str = EVM_DERIVATION_PATH;

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

    let assets = fetch_portfolio_assets(&addresses, &[]).await;

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

    let assets = fetch_portfolio_assets(&addresses, &[]).await;

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

    let (address, addresses, cached_assets) = {
        let mut state = state.lock().map_err(|_| "State lock failed")?;

        let in_memory = state.wallet.as_ref().map(|w| {
            (w.passphrase_hash.clone(), w.address.clone(), w.addresses.clone(), w.assets.clone())
        });

        if let Some((stored_hash, addr, addresses, assets)) = in_memory {
            if stored_hash != passphrase_hash {
                return Err("Invalid passphrase".to_string());
            }
            state.locked = false;
            (addr, addresses, assets)
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
            let addresses = wallet.addresses.clone();
            let assets = wallet.assets.clone();
            state.wallet = Some(wallet);
            state.locked = false;
            (address, addresses, assets)
        }
    };

    let mut refresh_addresses = addresses;
    refresh_addresses.entry("evm".to_string()).or_insert(address);
    let fresh_assets = fetch_portfolio_assets(&refresh_addresses, &cached_assets).await;

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if let Some(wallet) = state.wallet.as_mut() {
        wallet.assets = fresh_assets;
    }
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

fn clear_secret_string(s: &mut String) {
    let buf = unsafe { s.as_bytes_mut() };
    buf.fill(0);
}

#[tauri::command]
fn lock_wallet(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if let Some(ref mut wallet) = state.wallet {
        clear_secret_string(&mut wallet.mnemonic);
    }
    state.wallet = None;
    if let Some(ref mut key) = state.encryption_key {
        key.zeroize();
    }
    state.encryption_key = None;
    if let Some(ref mut salt) = state.storage_salt {
        salt.fill(0);
    }
    state.storage_salt = None;
    state.locked = true;
    Ok(())
}

#[tauri::command]
fn clear_wallet(state: State<'_, Mutex<AppState>>) -> Result<WalletSession, String> {
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if state.storage_path.exists() {
        fs::remove_file(&state.storage_path).map_err(|_| "Failed to remove stored wallet")?;
    }
    if let Some(ref mut wallet) = state.wallet {
        clear_secret_string(&mut wallet.mnemonic);
    }
    state.wallet = None;
    state.stored_wallet = None;
    if let Some(ref mut key) = state.encryption_key {
        key.zeroize();
    }
    state.encryption_key = None;
    if let Some(ref mut salt) = state.storage_salt {
        salt.fill(0);
    }
    state.storage_salt = None;
    state.locked = false;
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

    let (mnemonic, address, addresses, assets) = {
        let state = state.lock().map_err(|_| "State lock failed")?;
        let wallet = state
            .wallet
            .as_ref()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        validate_transfer(wallet, &to, &symbol, &amount)?;
        (
            wallet.mnemonic.clone(),
            wallet.address.clone(),
            wallet.addresses.clone(),
            wallet.assets.clone(),
        )
    };

    let to = to.trim().to_string();
    let value_wei: u128 = amount.parse().map_err(|_| "Invalid amount".to_string())?;

    let asset = assets.iter()
        .find(|a| a.symbol == symbol)
        .ok_or_else(|| format!("Asset {symbol} not found in wallet"))?;
    let network_id = &asset.network;
    let decimals = asset.decimals;

    if network_id == "bitcoin" && symbol == "BTC" {
        let from = addresses
            .get("bitcoin")
            .ok_or_else(|| "Wallet BTC address is not available".to_string())?
            .clone();
        let amount_sats: u64 = value_wei
            .try_into()
            .map_err(|_| "BTC amount is too large".to_string())?;
        let signed_btc = sign_bitcoin_transfer(&mnemonic, &from, &to, amount_sats).await?;
        return Ok(SignedTransaction {
            from,
            to,
            symbol: symbol.clone(),
            amount: value_wei.to_string(),
            note: note.trim().to_string(),
            network: "bitcoin".to_string(),
            nonce: "utxo".to_string(),
            signed_at: Utc::now().to_rfc3339(),
            payload_hash: signed_btc.txid.clone(),
            signature: signed_btc.first_signature_hex,
            fee_amount: signed_btc.fee_sats.to_string(),
            fee_symbol: "BTC".to_string(),
            total_debit: (amount_sats + signed_btc.fee_sats).to_string(),
            post_balance: signed_btc.post_balance.to_string(),
            decimals,
            fiat_value: 0.0,
            raw_tx: Some(signed_btc.raw_tx_hex),
            tx_hash: Some(signed_btc.txid),
        });
    }

    if evm_config_by_id(network_id).is_none() {
        return Err(format!(
            "{} transfers on {} are not implemented yet",
            symbol, network_id
        ));
    }

    let config = evm_config_by_id(network_id)
        .ok_or_else(|| format!("No EVM chain configured for network {network_id}"))?;

    let is_native = evm_config_for_symbol(&symbol).is_some();

    let (tx_to, tx_data, display_to) = if is_native {
        (to.clone(), Vec::new(), to.clone())
    } else {
        let token = evm_tokens_for_network(config.id).iter()
            .find(|t| t.symbol == symbol)
            .ok_or_else(|| format!("Token {symbol} not found on {network_id}"))?;
        (token.contract.to_string(), encode_erc20_transfer(&to, value_wei)?, to.clone())
    };

    let nonce = fetch_evm_nonce(config, &address).await?;
    let gas_price = fetch_evm_gas_price(config).await?;
    let gas_limit = if tx_data.is_empty() {
        fetch_evm_estimate_gas(config, &address, &tx_to, value_wei, &[]).await?
    } else {
        fetch_evm_estimate_gas(config, &address, &tx_to, 0, &tx_data).await?
    };

    let max_priority_fee_per_gas = gas_price;
    let max_fee_per_gas = gas_price;
    let total_fee_wei = gas_limit as u128 * max_fee_per_gas;

    let signing_key = signing_key_from_mnemonic(&mnemonic)?;
    let (_, tx_hash, raw_tx_hex, r_hex, s_hex) = sign_eip1559_transfer(
        &signing_key,
        config.chain_id,
        nonce,
        max_priority_fee_per_gas,
        max_fee_per_gas,
        gas_limit,
        &tx_to,
        if tx_data.is_empty() { value_wei } else { 0 },
        &tx_data,
    )?;

    let amount_str = value_wei.to_string();
    let fee_str = total_fee_wei.to_string();
    let total_debit_str = if is_native {
        (value_wei + total_fee_wei).to_string()
    } else {
        total_fee_wei.to_string()
    };
    let signature_str = format!("0x{}{}", r_hex, s_hex);

    let post_balance_wei: u128 = asset.balance.parse().unwrap_or(0);
    let post_balance = if post_balance_wei >= value_wei {
        (post_balance_wei - value_wei).to_string()
    } else {
        "0".to_string()
    };

    let signed = SignedTransaction {
        from: address,
        to: display_to,
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
        if signed.from != wallet.address && !wallet.addresses.values().any(|address| address == &signed.from) {
            return Err("Signed transaction does not match this wallet".to_string());
        }
    }

    let raw_tx = signed
        .raw_tx
        .as_ref()
        .ok_or_else(|| "No raw transaction data".to_string())?;

    let tx_hash = if signed.network == "bitcoin" && signed.symbol == "BTC" {
        broadcast_bitcoin_transaction(raw_tx).await?
    } else {
        let config = evm_config_for_symbol(&signed.symbol)
            .or_else(|| evm_config_by_id(&signed.network))
            .ok_or_else(|| format!("No EVM chain configured for {}", signed.symbol))?;
        broadcast_evm_transaction(config, raw_tx).await?
    };

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
        "BTC" => validate_bitcoin_address(address),
        "SOL" => validate_solana_address(address),
        "ZEC" => validate_zcash_address(address),
        "FIL" => validate_filecoin_address(address),
        "INJ" => validate_injective_address(address),
        _ => validate_evm_address(address),
    }
}

fn validate_evm_address(address: &str) -> Result<(), String> {
    let hex_part = address.strip_prefix("0x").unwrap_or(address);
    if hex_part.len() != 40 || !hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Recipient must be a valid 0x-prefixed 40-hex-char EVM address".to_string());
    }

    let has_lower = hex_part.chars().any(|c| c.is_ascii_lowercase());
    let has_upper = hex_part.chars().any(|c| c.is_ascii_uppercase());
    if has_lower && has_upper {
        let hex_lower = hex_part.to_lowercase();
        let hash = Keccak256::digest(hex_lower.as_bytes());
        let hash_hex = hex::encode(hash);
        for (i, c) in hex_part.chars().enumerate() {
            if c.is_ascii_digit() {
                continue;
            }
            let nibble = u8::from_str_radix(&hash_hex[i..i + 1], 16).unwrap_or(0);
            let should_be_upper = nibble >= 8;
            if should_be_upper != c.is_ascii_uppercase() {
                return Err("EIP-55 checksum validation failed".to_string());
            }
        }
    }

    Ok(())
}

fn validate_bitcoin_address(address: &str) -> Result<(), String> {
    if address.starts_with("bc1") || address.starts_with("tb1") {
        bech32::decode(address).map_err(|_| "Recipient must be a valid Bitcoin bech32 address".to_string())?;
        return Ok(());
    }
    if address.starts_with('1') || address.starts_with('3') || address.starts_with('2') || address.starts_with('m') || address.starts_with('n') {
        bs58::decode(address).with_check(None).into_vec()
            .map_err(|_| "Recipient must be a valid Bitcoin base58 address".to_string())?;
        return Ok(());
    }
    Err("Recipient must be a valid Bitcoin address (bc1, 1, or 3)".to_string())
}

fn validate_solana_address(address: &str) -> Result<(), String> {
    let bytes = bs58::decode(address).into_vec()
        .map_err(|_| "Recipient must be a valid base58 Solana address".to_string())?;
    if bytes.len() != 32 {
        return Err("Solana address must decode to 32 bytes".to_string());
    }
    Ok(())
}

fn validate_zcash_address(address: &str) -> Result<(), String> {
    if address.starts_with("zs1") || address.starts_with("ztestsapling") {
        return Err("Zcash shielded addresses are not yet supported".to_string());
    }
    if address.starts_with("t1") || address.starts_with("t3") || address.starts_with("tm") {
        let bytes = bs58::decode(address).into_vec()
            .map_err(|_| "Recipient must be a valid Zcash transparent address".to_string())?;
        if bytes.len() != 26 {
            return Err("Zcash transparent address must decode to 26 bytes".to_string());
        }
        let payload = &bytes[..22];
        let checksum = &bytes[22..];
        let hash = Sha256::digest(&Sha256::digest(payload));
        if &hash[..4] != checksum {
            return Err("Zcash transparent address checksum invalid".to_string());
        }
        return Ok(());
    }
    Err("Recipient must be a valid Zcash address (t1 or tm)".to_string())
}

fn validate_filecoin_address(address: &str) -> Result<(), String> {
    if !address.starts_with('f') && !address.starts_with('t') {
        return Err("Filecoin address must start with f or t".to_string());
    }
    if address.len() < 3 {
        return Err("Filecoin address too short".to_string());
    }
    let protocol = address.chars().nth(1).unwrap_or(' ');
    match protocol {
        '0' => {
            if !address[2..].chars().all(|c| c.is_ascii_digit()) {
                return Err("Filecoin ID address must contain only digits after f0".to_string());
            }
            Ok(())
        }
        '1' => {
            let bytes = bs58::decode(&address[2..]).with_check(Some(0x01)).into_vec()
                .map_err(|_| "Invalid Filecoin f1 address".to_string())?;
            if bytes.len() != 21 {
                return Err("Filecoin f1 address must decode to 21 bytes (1 prefix + 20 payload)".to_string());
            }
            if bytes[0] != 1 {
                return Err("Filecoin f1 address has wrong protocol byte".to_string());
            }
            Ok(())
        }
        '3' => {
            let bytes = bs58::decode(&address[2..]).with_check(Some(0x03)).into_vec()
                .map_err(|_| "Invalid Filecoin f3 address".to_string())?;
            if bytes.len() != 48 {
                return Err("Filecoin f3 (BLS) address must decode to 48 bytes".to_string());
            }
            Ok(())
        }
        '4' => {
            if address.starts_with("f410") || address.starts_with("t410") {
                bech32::decode(address).map_err(|_| "Invalid Filecoin f4 (delegated) address".to_string())?;
                Ok(())
            } else {
                Err("Filecoin f4 address must start with f410".to_string())
            }
        }
        _ => Err("Unknown Filecoin address protocol".to_string()),
    }
}

fn validate_injective_address(address: &str) -> Result<(), String> {
    if !address.starts_with("inj1") {
        return Err("Injective address must start with inj1".to_string());
    }
    bech32::decode(address).map_err(|_| "Recipient must be a valid Injective bech32 address".to_string())?;
    Ok(())
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

fn evm_config_by_id(network_id: &str) -> Option<&'static EvmNetworkConfig> {
    EVM_NETWORKS.iter().find(|c| c.id == network_id)
}

#[allow(dead_code)]
fn evm_network_id_for_token(symbol: &str) -> Option<&'static str> {
    EVM_TOKENS.iter()
        .find(|(_, tokens)| tokens.iter().any(|t| t.symbol == symbol))
        .map(|(id, _)| *id)
}

fn encode_erc20_transfer(recipient: &str, amount: u128) -> Result<Vec<u8>, String> {
    let recip_hex = recipient.trim_start_matches("0x");
    let recip_bytes = hex::decode(recip_hex).map_err(|_| "Invalid recipient address".to_string())?;
    let mut padded_recip = vec![0u8; 32];
    padded_recip[32 - recip_bytes.len()..].copy_from_slice(&recip_bytes);

    let amount_bytes = amount.to_be_bytes();
    let start = amount_bytes.iter().position(|&b| b != 0).unwrap_or(amount_bytes.len() - 1);
    let amount_trimmed = &amount_bytes[start..];

    let mut data = vec![0xa9, 0x05, 0x9c, 0xbb]; // keccak256("transfer(address,uint256)")[..4]
    data.extend_from_slice(&padded_recip);
    let mut padded_amount = vec![0u8; 32];
    padded_amount[32 - amount_trimmed.len()..].copy_from_slice(amount_trimmed);
    data.extend_from_slice(&padded_amount);
    Ok(data)
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

    let json = rpc_post(config.rpc_url, &body).await?;
    let balance_hex = json["result"]
        .as_str()
        .ok_or_else(|| "RPC response missing result field".to_string())?;

    u128::from_str_radix(balance_hex.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid balance hex: {e}"))
}

fn cached_asset(cached_assets: &[Asset], network_id: &str, symbol: &str) -> Option<Asset> {
    cached_assets
        .iter()
        .find(|asset| asset.network == network_id && asset.symbol == symbol)
        .cloned()
}

async fn fetch_portfolio_assets(addresses: &HashMap<String, String>, cached_assets: &[Asset]) -> Vec<Asset> {
    let mut assets = vec![];

    if let Some(evm_address) = addresses.get("evm") {
        assets.extend(fetch_evm_assets(DEFAULT_EVM_CONFIG, evm_address, cached_assets).await);
    }

    for config in NON_EVM_NATIVE_ASSETS {
        let Some(address) = addresses.get(config.address_key) else {
            continue;
        };

        match fetch_non_evm_native_asset(config, address).await {
            Ok(asset) => assets.push(asset),
            Err(_) => {
                if let Some(cached) = cached_asset(cached_assets, config.network_id, config.symbol) {
                    assets.push(cached);
                }
            }
        }
    }

    assets
}

async fn fetch_non_evm_native_asset(config: &NativeAssetConfig, address: &str) -> Result<Asset, String> {
    let balance = match config.symbol {
        "BTC" => fetch_bitcoin_balance(address).await?,
        "SOL" => fetch_solana_balance(address).await?,
        _ => return Err(format!("{} provider is not implemented yet", config.symbol)),
    };

    Ok(Asset {
        symbol: config.symbol.to_string(),
        name: config.name.to_string(),
        balance,
        decimals: config.decimals,
        price_usd: 0.0,
        change_24h: 0.0,
        network: config.network_id.to_string(),
    })
}

async fn fetch_bitcoin_balance(address: &str) -> Result<String, String> {
    let url = format!("https://blockstream.info/api/address/{address}");
    let json = http_get_json(&url).await?;
    parse_bitcoin_balance(&json).map(|sats| sats.to_string())
}

fn parse_bitcoin_balance(json: &serde_json::Value) -> Result<u128, String> {
    let funded = json["chain_stats"]["funded_txo_sum"].as_u64().unwrap_or(0) as u128;
    let spent = json["chain_stats"]["spent_txo_sum"].as_u64().unwrap_or(0) as u128;
    let mempool_funded = json["mempool_stats"]["funded_txo_sum"].as_u64().unwrap_or(0) as u128;
    let mempool_spent = json["mempool_stats"]["spent_txo_sum"].as_u64().unwrap_or(0) as u128;

    let confirmed = funded.saturating_sub(spent);
    let mempool = mempool_funded.saturating_sub(mempool_spent);
    Ok(confirmed + mempool)
}

async fn fetch_solana_balance(address: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getBalance",
        "params": [address],
        "id": 1,
    });
    let json = rpc_post("https://api.mainnet-beta.solana.com", &body).await?;
    parse_solana_balance(&json).map(|lamports| lamports.to_string())
}

fn parse_solana_balance(json: &serde_json::Value) -> Result<u128, String> {
    json["result"]["value"]
        .as_u64()
        .map(|value| value as u128)
        .ok_or_else(|| "Solana balance RPC missing result.value".to_string())
}

async fn fetch_bitcoin_utxos(address: &str) -> Result<Vec<BitcoinUtxo>, String> {
    let url = format!("https://blockstream.info/api/address/{address}/utxo");
    let json = http_get_json(&url).await?;
    parse_bitcoin_utxos(&json)
}

fn parse_bitcoin_utxos(json: &serde_json::Value) -> Result<Vec<BitcoinUtxo>, String> {
    let arr = json.as_array().ok_or_else(|| "Bitcoin UTXO response is not an array".to_string())?;
    let mut utxos = vec![];
    for item in arr {
        let txid = item["txid"]
            .as_str()
            .ok_or_else(|| "Bitcoin UTXO missing txid".to_string())?
            .to_string();
        if txid.len() != 64 || !txid.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("Bitcoin UTXO txid is invalid".to_string());
        }
        let vout = item["vout"].as_u64().ok_or_else(|| "Bitcoin UTXO missing vout".to_string())?;
        let value = item["value"].as_u64().ok_or_else(|| "Bitcoin UTXO missing value".to_string())?;
        if value < 546 {
            continue;
        }
        utxos.push(BitcoinUtxo {
            txid,
            vout: u32::try_from(vout).map_err(|_| "Bitcoin UTXO vout is too large".to_string())?,
            value,
            confirmed: item["status"]["confirmed"].as_bool().unwrap_or(false),
        });
    }
    utxos.sort_by(|a, b| b.confirmed.cmp(&a.confirmed).then(a.value.cmp(&b.value)));
    Ok(utxos)
}

async fn fetch_bitcoin_fee_rate() -> Result<u64, String> {
    let json = http_get_json("https://blockstream.info/api/fee-estimates").await?;
    parse_bitcoin_fee_rate(&json)
}

fn parse_bitcoin_fee_rate(json: &serde_json::Value) -> Result<u64, String> {
    for target in ["3", "6", "12", "1"] {
        if let Some(rate) = json[target].as_f64() {
            if rate.is_finite() && rate > 0.0 {
                return Ok(rate.ceil().max(1.0) as u64);
            }
        }
    }
    Err("Bitcoin fee estimate response missing usable fee rate".to_string())
}

fn bitcoin_varint(value: u64) -> Vec<u8> {
    if value < 0xfd {
        vec![value as u8]
    } else if value <= 0xffff {
        let mut out = vec![0xfd];
        out.extend_from_slice(&(value as u16).to_le_bytes());
        out
    } else if value <= 0xffff_ffff {
        let mut out = vec![0xfe];
        out.extend_from_slice(&(value as u32).to_le_bytes());
        out
    } else {
        let mut out = vec![0xff];
        out.extend_from_slice(&value.to_le_bytes());
        out
    }
}

fn bitcoin_push_data(data: &[u8]) -> Vec<u8> {
    let mut out = bitcoin_varint(data.len() as u64);
    out.extend_from_slice(data);
    out
}

fn bitcoin_p2wpkh_script_pubkey(pubkey_hash: &[u8]) -> Vec<u8> {
    let mut script = vec![0x00, 0x14];
    script.extend_from_slice(pubkey_hash);
    script
}

fn bitcoin_p2pkh_script_code(pubkey_hash: &[u8]) -> Vec<u8> {
    let mut script = vec![0x76, 0xa9, 0x14];
    script.extend_from_slice(pubkey_hash);
    script.extend_from_slice(&[0x88, 0xac]);
    script
}

fn bitcoin_script_pubkey_from_address(address: &str) -> Result<Vec<u8>, String> {
    if address.starts_with("bc1") {
        let (hrp, data, variant) = bech32::decode(address)
            .map_err(|_| "Invalid Bitcoin bech32 recipient".to_string())?;
        if hrp != "bc" || variant != Variant::Bech32 || data.is_empty() {
            return Err("Unsupported Bitcoin bech32 recipient".to_string());
        }
        let version = data[0].to_u8();
        let program = Vec::<u8>::from_base32(&data[1..])
            .map_err(|_| "Invalid Bitcoin witness program".to_string())?;
        if version != 0 || program.len() != 20 {
            return Err("Only mainnet P2WPKH bc1 recipients are supported".to_string());
        }
        return Ok(bitcoin_p2wpkh_script_pubkey(&program));
    }

    let decoded = bs58::decode(address).with_check(None).into_vec()
        .map_err(|_| "Invalid Bitcoin base58 recipient".to_string())?;
    if decoded.len() != 21 {
        return Err("Unsupported Bitcoin base58 recipient length".to_string());
    }
    let version = decoded[0];
    let hash = &decoded[1..];
    match version {
        0x00 => {
            let mut script = vec![0x76, 0xa9, 0x14];
            script.extend_from_slice(hash);
            script.extend_from_slice(&[0x88, 0xac]);
            Ok(script)
        }
        0x05 => {
            let mut script = vec![0xa9, 0x14];
            script.extend_from_slice(hash);
            script.push(0x87);
            Ok(script)
        }
        _ => Err("Only mainnet Bitcoin recipients are supported".to_string()),
    }
}

fn bitcoin_txid_le(txid: &str) -> Result<Vec<u8>, String> {
    let mut bytes = hex::decode(txid).map_err(|_| "Invalid Bitcoin txid hex".to_string())?;
    if bytes.len() != 32 {
        return Err("Bitcoin txid must be 32 bytes".to_string());
    }
    bytes.reverse();
    Ok(bytes)
}

fn bitcoin_double_sha256(data: &[u8]) -> [u8; 32] {
    let first = Sha256::digest(data);
    Sha256::digest(first).into()
}

fn bitcoin_txid_from_stripped(stripped_tx: &[u8]) -> String {
    let mut hash = bitcoin_double_sha256(stripped_tx);
    hash.reverse();
    hex::encode(hash)
}

fn bitcoin_serialize_outputs(outputs: &[BitcoinTxOutput]) -> Vec<u8> {
    let mut out = bitcoin_varint(outputs.len() as u64);
    for output in outputs {
        out.extend_from_slice(&output.value.to_le_bytes());
        out.extend(bitcoin_push_data(&output.script_pubkey));
    }
    out
}

fn bitcoin_serialize_stripped(inputs: &[BitcoinTxInput], outputs: &[BitcoinTxOutput]) -> Result<Vec<u8>, String> {
    let mut tx = vec![];
    tx.extend_from_slice(&2i32.to_le_bytes());
    tx.extend(bitcoin_varint(inputs.len() as u64));
    for input in inputs {
        tx.extend(bitcoin_txid_le(&input.utxo.txid)?);
        tx.extend_from_slice(&input.utxo.vout.to_le_bytes());
        tx.push(0x00);
        tx.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
    }
    tx.extend(bitcoin_serialize_outputs(outputs));
    tx.extend_from_slice(&0u32.to_le_bytes());
    Ok(tx)
}

fn bitcoin_sighash(input_index: usize, inputs: &[BitcoinTxInput], outputs: &[BitcoinTxOutput]) -> Result<[u8; 32], String> {
    let mut prevouts = vec![];
    let mut sequences = vec![];
    for input in inputs {
        prevouts.extend(bitcoin_txid_le(&input.utxo.txid)?);
        prevouts.extend_from_slice(&input.utxo.vout.to_le_bytes());
        sequences.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
    }

    let hash_prevouts = bitcoin_double_sha256(&prevouts);
    let hash_sequence = bitcoin_double_sha256(&sequences);
    let hash_outputs = bitcoin_double_sha256(&bitcoin_serialize_outputs(outputs));
    let input = inputs.get(input_index).ok_or_else(|| "Bitcoin input index out of range".to_string())?;

    let mut preimage = vec![];
    preimage.extend_from_slice(&2i32.to_le_bytes());
    preimage.extend_from_slice(&hash_prevouts);
    preimage.extend_from_slice(&hash_sequence);
    preimage.extend(bitcoin_txid_le(&input.utxo.txid)?);
    preimage.extend_from_slice(&input.utxo.vout.to_le_bytes());
    preimage.extend(bitcoin_push_data(&input.script_code));
    preimage.extend_from_slice(&input.utxo.value.to_le_bytes());
    preimage.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
    preimage.extend_from_slice(&hash_outputs);
    preimage.extend_from_slice(&0u32.to_le_bytes());
    preimage.extend_from_slice(&1u32.to_le_bytes());
    Ok(bitcoin_double_sha256(&preimage))
}

fn bitcoin_estimated_vbytes(input_count: usize, output_count: usize) -> u64 {
    10 + (input_count as u64 * 68) + (output_count as u64 * 34)
}

fn bitcoin_select_coins(utxos: &[BitcoinUtxo], amount: u64, fee_rate_sat_vb: u64) -> Result<(Vec<BitcoinUtxo>, u64, u64), String> {
    let mut selected = vec![];
    let mut total = 0u64;
    for utxo in utxos.iter().filter(|u| u.confirmed).chain(utxos.iter().filter(|u| !u.confirmed)) {
        selected.push(utxo.clone());
        total = total.saturating_add(utxo.value);
        let fee_with_change = bitcoin_estimated_vbytes(selected.len(), 2).saturating_mul(fee_rate_sat_vb);
        if total >= amount.saturating_add(fee_with_change) {
            let change = total - amount - fee_with_change;
            if change < 546 {
                let fee_no_change = bitcoin_estimated_vbytes(selected.len(), 1).saturating_mul(fee_rate_sat_vb);
                if total >= amount.saturating_add(fee_no_change) {
                    return Ok((selected, total - amount, 0));
                }
            }
            return Ok((selected, fee_with_change, change));
        }
    }
    Err("Insufficient BTC balance for amount plus fee".to_string())
}

fn bitcoin_signed_transfer(
    private_key: &[u8; 32],
    from_address: &str,
    to_address: &str,
    amount_sats: u64,
    utxos: &[BitcoinUtxo],
    fee_rate_sat_vb: u64,
) -> Result<BitcoinSignedTransfer, String> {
    if amount_sats == 0 {
        return Err("Amount must be greater than zero".to_string());
    }

    let signing_key = signing_key_from_private_key(private_key)?;
    let public_key = signing_key.verifying_key().to_encoded_point(true);
    let public_key_bytes = public_key.as_bytes();
    let pubkey_hash = Ripemd160::digest(&Sha256::digest(public_key_bytes));
    let expected_from = bitcoin_bech32_address(private_key, false)?;
    if from_address != expected_from {
        return Err("Derived BTC key does not match wallet BTC address".to_string());
    }

    let (selected, fee_sats, change_sats) = bitcoin_select_coins(utxos, amount_sats, fee_rate_sat_vb)?;
    let total_in: u64 = selected.iter().map(|u| u.value).sum();
    let mut outputs = vec![BitcoinTxOutput {
        value: amount_sats,
        script_pubkey: bitcoin_script_pubkey_from_address(to_address)?,
    }];
    if change_sats > 0 {
        outputs.push(BitcoinTxOutput {
            value: change_sats,
            script_pubkey: bitcoin_p2wpkh_script_pubkey(&pubkey_hash),
        });
    }

    let script_code = bitcoin_p2pkh_script_code(&pubkey_hash);
    let inputs: Vec<BitcoinTxInput> = selected
        .into_iter()
        .map(|utxo| BitcoinTxInput { utxo, script_code: script_code.clone() })
        .collect();

    let mut signatures = vec![];
    for i in 0..inputs.len() {
        let sighash = bitcoin_sighash(i, &inputs, &outputs)?;
        let signature: k256::ecdsa::Signature = signing_key
            .sign_prehash(&sighash)
            .map_err(|_| "Bitcoin transaction signing failed".to_string())?;
        let mut der = signature.to_der().as_bytes().to_vec();
        der.push(0x01);
        signatures.push(der);
    }

    let stripped = bitcoin_serialize_stripped(&inputs, &outputs)?;
    let txid = bitcoin_txid_from_stripped(&stripped);

    let mut raw = vec![];
    raw.extend_from_slice(&2i32.to_le_bytes());
    raw.extend_from_slice(&[0x00, 0x01]);
    raw.extend(bitcoin_varint(inputs.len() as u64));
    for input in &inputs {
        raw.extend(bitcoin_txid_le(&input.utxo.txid)?);
        raw.extend_from_slice(&input.utxo.vout.to_le_bytes());
        raw.push(0x00);
        raw.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
    }
    raw.extend(bitcoin_serialize_outputs(&outputs));
    for sig in &signatures {
        raw.push(0x02);
        raw.extend(bitcoin_push_data(sig));
        raw.extend(bitcoin_push_data(public_key_bytes));
    }
    raw.extend_from_slice(&0u32.to_le_bytes());

    Ok(BitcoinSignedTransfer {
        txid,
        raw_tx_hex: hex::encode(raw),
        first_signature_hex: signatures.first().map(hex::encode).unwrap_or_default(),
        fee_sats,
        post_balance: total_in.saturating_sub(amount_sats).saturating_sub(fee_sats),
    })
}

async fn sign_bitcoin_transfer(mnemonic: &str, from: &str, to: &str, amount_sats: u64) -> Result<BitcoinSignedTransfer, String> {
    let private_key = secp256k1_private_key_from_mnemonic(mnemonic, BITCOIN_DERIVATION_PATH)?;
    let utxos = fetch_bitcoin_utxos(from).await?;
    let fee_rate = fetch_bitcoin_fee_rate().await?;
    bitcoin_signed_transfer(&private_key, from, to, amount_sats, &utxos, fee_rate)
}

async fn broadcast_bitcoin_transaction(raw_tx_hex: &str) -> Result<String, String> {
    http_post_text("https://blockstream.info/api/tx", raw_tx_hex)
        .await
        .map(|txid| txid.trim().to_string())
}

async fn fetch_bitcoin_tx_status(txid: &str) -> Result<Option<String>, String> {
    let url = format!("https://blockstream.info/api/tx/{txid}/status");
    let json = http_get_json(&url).await?;
    if json["confirmed"].as_bool().unwrap_or(false) {
        Ok(Some("confirmed".to_string()))
    } else {
        Ok(None)
    }
}

async fn fetch_evm_assets(config: &EvmNetworkConfig, address: &str, cached_assets: &[Asset]) -> Vec<Asset> {
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
        Err(_) => cached_asset(cached_assets, config.id, config.native_symbol).unwrap_or_else(|| Asset {
            symbol: config.native_symbol.to_string(),
            name: config.display_name.to_string(),
            balance: "0".to_string(),
            decimals: 18,
            price_usd: 0.0,
            change_24h: 0.0,
            network: config.id.to_string(),
        }),
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
            Err(_) => {
                if let Some(cached) = cached_asset(cached_assets, config.id, token.symbol) {
                    assets.push(cached);
                }
            }
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
    let json = rpc_post(config.rpc_url, &body).await?;
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
    let json = rpc_post(config.rpc_url, &body).await?;
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
    let json = rpc_post(config.rpc_url, &body).await?;
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
    let json = rpc_post(config.rpc_url, &body).await?;
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
    let json = rpc_post(config.rpc_url, &body).await?;

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
    if network == "bitcoin" {
        return fetch_bitcoin_tx_status(&tx_hash).await;
    }

    let config = EVM_NETWORKS.iter().find(|c| c.id == network)
        .ok_or_else(|| format!("Unknown network: {}", network))?;
    fetch_tx_status(config, &tx_hash).await
}

fn signing_key_from_mnemonic(mnemonic: &str) -> Result<k256::ecdsa::SigningKey, String> {
    let private_key = secp256k1_private_key_from_mnemonic(mnemonic, EVM_DERIVATION_PATH)?;
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
    data: &[u8],
) -> Result<(Vec<u8>, String, String, String, String), String> {
    let to_bytes = hex::decode(to.trim_start_matches("0x"))
        .map_err(|_| "Invalid to address".to_string())?;

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
    stream.append(&data.to_vec());
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
    tx_stream.append(&data.to_vec());
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

fn mnemonic_seed(mnemonic: &str) -> Result<[u8; 64], String> {
    let mnemonic = Mnemonic::parse_in_normalized(Language::English, mnemonic)
        .map_err(|_| "Invalid recovery phrase".to_string())?;
    Ok(mnemonic.to_seed(""))
}

fn secp256k1_private_key_from_mnemonic(mnemonic: &str, path: &str) -> Result<[u8; 32], String> {
    let seed = mnemonic_seed(mnemonic)?;
    let path: DerivationPath = path.parse()
        .map_err(|_| format!("Invalid derivation path: {path}"))?;
    let child = XPrv::derive_from_path(&seed, &path)
        .map_err(|_| format!("Failed to derive key at {path}"))?;
    let bytes = child.private_key().to_bytes();
    Ok(bytes.into())
}

fn solana_secret_key_from_mnemonic(mnemonic: &str) -> Result<[u8; 32], String> {
    type HmacSha512 = Hmac<Sha512>;

    let seed = mnemonic_seed(mnemonic)?;
    let mut mac = <HmacSha512 as Mac>::new_from_slice(b"ed25519 seed")
        .map_err(|_| "Failed to initialize Solana derivation".to_string())?;
    mac.update(&seed);
    let result = mac.finalize().into_bytes();
    let mut key = [0u8; 32];
    let mut chain_code = [0u8; 32];
    key.copy_from_slice(&result[..32]);
    chain_code.copy_from_slice(&result[32..]);

    for index in SOLANA_DERIVATION_PATH {
        let hardened = index | 0x8000_0000;
        let mut data = Vec::with_capacity(37);
        data.push(0);
        data.extend_from_slice(&key);
        data.extend_from_slice(&hardened.to_be_bytes());

        let mut mac = <HmacSha512 as Mac>::new_from_slice(&chain_code)
            .map_err(|_| "Failed to derive Solana child key".to_string())?;
        mac.update(&data);
        let result = mac.finalize().into_bytes();
        key.copy_from_slice(&result[..32]);
        chain_code.copy_from_slice(&result[32..]);
    }

    Ok(key)
}

fn derive_addresses_from_mnemonic(mnemonic: &str) -> Result<HashMap<String, String>, String> {
    let evm_private_key = secp256k1_private_key_from_mnemonic(mnemonic, EVM_DERIVATION_PATH)?;
    let bitcoin_private_key = secp256k1_private_key_from_mnemonic(mnemonic, BITCOIN_DERIVATION_PATH)?;
    let zcash_private_key = secp256k1_private_key_from_mnemonic(mnemonic, ZCASH_DERIVATION_PATH)?;
    let solana_secret_key = solana_secret_key_from_mnemonic(mnemonic)?;
    let filecoin_private_key = secp256k1_private_key_from_mnemonic(mnemonic, FILECOIN_DERIVATION_PATH)?;
    let injective_private_key = secp256k1_private_key_from_mnemonic(mnemonic, INJECTIVE_DERIVATION_PATH)?;

    let evm_address = ethereum_address_from_private_key(&evm_private_key)?;
    let bitcoin_address = bitcoin_bech32_address(&bitcoin_private_key, false)?;
    let zcash_address = zcash_transparent_address(&zcash_private_key, false)?;
    let solana_address = solana_address_from_secret_key(&solana_secret_key)?;
    let filecoin_address = filecoin_address_from_private_key(&filecoin_private_key)?;
    let injective_address = bech32_account_address(&injective_private_key, "inj")?;

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

fn solana_address_from_secret_key(secret_bytes: &[u8; 32]) -> Result<String, String> {
    let secret = DalekSecretKey::from_bytes(secret_bytes)
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
    Ok(format!("f1{}", bs58::encode(bytes).with_check().into_string()))
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

#[allow(dead_code)]
trait ChainProvider: Send + Sync {
    fn chain_name(&self) -> &'static str;
    fn symbol(&self) -> &'static str;
    fn validate_address(&self, address: &str) -> Result<(), String>;
    fn derive_address(&self, private_key: &[u8; 32]) -> Result<String, String>;
}

struct EvmProvider;
struct BitcoinProvider;
struct SolanaProvider;
struct ZcashProvider;
struct FilecoinProvider;
struct InjectiveProvider;

impl ChainProvider for EvmProvider {
    fn chain_name(&self) -> &'static str { "EVM" }
    fn symbol(&self) -> &'static str { "ETH" }
    fn validate_address(&self, address: &str) -> Result<(), String> { validate_evm_address(address) }
    fn derive_address(&self, private_key: &[u8; 32]) -> Result<String, String> { ethereum_address_from_private_key(private_key) }
}

impl ChainProvider for BitcoinProvider {
    fn chain_name(&self) -> &'static str { "Bitcoin" }
    fn symbol(&self) -> &'static str { "BTC" }
    fn validate_address(&self, address: &str) -> Result<(), String> { validate_bitcoin_address(address) }
    fn derive_address(&self, private_key: &[u8; 32]) -> Result<String, String> { bitcoin_bech32_address(private_key, false) }
}

impl ChainProvider for SolanaProvider {
    fn chain_name(&self) -> &'static str { "Solana" }
    fn symbol(&self) -> &'static str { "SOL" }
    fn validate_address(&self, address: &str) -> Result<(), String> { validate_solana_address(address) }
    fn derive_address(&self, _private_key: &[u8; 32]) -> Result<String, String> {
        Err("Solana derivation requires seed bytes, not secp256k1 key".to_string())
    }
}

impl ChainProvider for ZcashProvider {
    fn chain_name(&self) -> &'static str { "Zcash" }
    fn symbol(&self) -> &'static str { "ZEC" }
    fn validate_address(&self, address: &str) -> Result<(), String> { validate_zcash_address(address) }
    fn derive_address(&self, private_key: &[u8; 32]) -> Result<String, String> { zcash_transparent_address(private_key, false) }
}

impl ChainProvider for FilecoinProvider {
    fn chain_name(&self) -> &'static str { "Filecoin" }
    fn symbol(&self) -> &'static str { "FIL" }
    fn validate_address(&self, address: &str) -> Result<(), String> { validate_filecoin_address(address) }
    fn derive_address(&self, private_key: &[u8; 32]) -> Result<String, String> { filecoin_address_from_private_key(private_key) }
}

impl ChainProvider for InjectiveProvider {
    fn chain_name(&self) -> &'static str { "Injective" }
    fn symbol(&self) -> &'static str { "INJ" }
    fn validate_address(&self, address: &str) -> Result<(), String> { validate_injective_address(address) }
    fn derive_address(&self, private_key: &[u8; 32]) -> Result<String, String> { bech32_account_address(private_key, "inj") }
}

#[allow(dead_code)]
fn get_provider(symbol: &str) -> Option<Box<dyn ChainProvider>> {
    match symbol {
        "BTC" => Some(Box::new(BitcoinProvider)),
        "SOL" => Some(Box::new(SolanaProvider)),
        "ZEC" => Some(Box::new(ZcashProvider)),
        "FIL" => Some(Box::new(FilecoinProvider)),
        "INJ" => Some(Box::new(InjectiveProvider)),
        _ => Some(Box::new(EvmProvider)),
    }
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
        assert!(validate_address_for_symbol("0xdAC17F958D2ee523a2206206994597C13D831ec7", "ETH").is_ok());
        assert!(validate_address_for_symbol("0xdac17f958d2ee523a2206206994597c13d831ec7", "ETH").is_ok());
        assert!(validate_address_for_symbol("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq", "BTC").is_ok());
        assert!(validate_address_for_symbol("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa", "BTC").is_ok());
        assert!(validate_address_for_symbol("3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy", "BTC").is_ok());
        assert!(validate_address_for_symbol("7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ", "SOL").is_ok());
        assert!(validate_address_for_symbol("t1eB29zcZ2v3AQvAEtcNrERsWQPmxyTN4DF", "ZEC").is_ok());
        assert!(validate_address_for_symbol("f1ke28mVhmmiSdiFRybu3ak3NnEqpx3o3Bk", "FIL").is_ok());
        assert!(validate_address_for_symbol("inj1m6kmamcpqgpsgpgxquyqjyq3zgf3g9gkzz8lqn", "INJ").is_ok());
        assert!(validate_address_for_symbol("0xdAC17F958D2ee523a2206206994597C13D831ec7", "MATIC").is_ok());
        assert!(validate_address_for_symbol("0xinvalid", "ETH").is_err());
        assert!(validate_address_for_symbol("bc1q", "BTC").is_err());
        assert!(validate_address_for_symbol("invalid", "SOL").is_err());
    }

    #[test]
    fn validates_eip55_checksum() {
        assert!(validate_evm_address("0xdAC17F958D2ee523a2206206994597C13D831ec7").is_ok());
        assert!(validate_evm_address("0xdac17f958d2ee523a2206206994597c13d831ec7").is_ok());
        assert!(validate_evm_address("0xDAc17f958D2eE523a2206206994597C13D831ec7").is_err());
        assert!(validate_evm_address("0xDbC17F958D2ee523a2206206994597C13D831ec7").is_err());
        assert!(validate_evm_address("0x0000000000000000000000000000000000000000").is_ok());
        assert!(validate_evm_address("0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF").is_ok());
    }

    #[test]
    fn provider_trait_covers_all_chains() {
        for symbol in &["ETH", "BTC", "SOL", "ZEC", "FIL", "INJ", "MATIC"] {
            let provider = get_provider(symbol);
            assert!(provider.is_some(), "No provider for symbol {symbol}");
        }
    }

    #[test]
    fn selects_cached_asset_by_network_and_symbol() {
        let assets = starter_assets("ethereum");
        let cached = cached_asset(&assets, "ethereum", "ETH").unwrap();
        assert_eq!(cached.symbol, "ETH");
        assert_eq!(cached.network, "ethereum");
        assert_eq!(cached.balance, "2482100000000000000000");
        assert!(cached_asset(&assets, "polygon", "ETH").is_none());
        assert!(cached_asset(&assets, "ethereum", "MATIC").is_none());
    }

    #[test]
    fn parses_bitcoin_balance_with_mempool_values() {
        let json = serde_json::json!({
            "chain_stats": {
                "funded_txo_sum": 5000,
                "spent_txo_sum": 1200
            },
            "mempool_stats": {
                "funded_txo_sum": 700,
                "spent_txo_sum": 200
            }
        });
        assert_eq!(parse_bitcoin_balance(&json).unwrap(), 4300);
    }

    #[test]
    fn parses_bitcoin_utxos_and_fee_rate() {
        let json = serde_json::json!([
            {
                "txid": "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                "vout": 1,
                "value": 50_000,
                "status": { "confirmed": true }
            },
            {
                "txid": "101102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e2f",
                "vout": 0,
                "value": 100,
                "status": { "confirmed": true }
            }
        ]);
        let utxos = parse_bitcoin_utxos(&json).unwrap();
        assert_eq!(utxos.len(), 1);
        assert_eq!(utxos[0].value, 50_000);

        let fees = serde_json::json!({ "3": 2.1, "6": 1.4 });
        assert_eq!(parse_bitcoin_fee_rate(&fees).unwrap(), 3);
    }

    #[test]
    fn selects_bitcoin_coins_with_change() {
        let utxos = vec![BitcoinUtxo {
            txid: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f".to_string(),
            vout: 0,
            value: 50_000,
            confirmed: true,
        }];
        let (selected, fee, change) = bitcoin_select_coins(&utxos, 10_000, 2).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(fee, bitcoin_estimated_vbytes(1, 2) * 2);
        assert_eq!(change, 50_000 - 10_000 - fee);
    }

    #[test]
    fn signs_bitcoin_p2wpkh_transfer() {
        let private_key = [0x01u8; 32];
        let from = bitcoin_bech32_address(&private_key, false).unwrap();
        let utxos = vec![BitcoinUtxo {
            txid: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f".to_string(),
            vout: 0,
            value: 50_000,
            confirmed: true,
        }];
        let signed = bitcoin_signed_transfer(
            &private_key,
            &from,
            "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq",
            10_000,
            &utxos,
            2,
        ).unwrap();

        assert_eq!(signed.txid.len(), 64);
        assert!(signed.raw_tx_hex.starts_with("020000000001"));
        assert!(!signed.first_signature_hex.is_empty());
        assert_eq!(signed.fee_sats, bitcoin_estimated_vbytes(1, 2) * 2);
        assert_eq!(signed.post_balance, 50_000 - 10_000 - signed.fee_sats);
    }

    #[test]
    fn parses_solana_balance_lamports() {
        let json = serde_json::json!({
            "jsonrpc": "2.0",
            "result": {
                "context": { "slot": 1 },
                "value": 123456789u64
            },
            "id": 1
        });
        assert_eq!(parse_solana_balance(&json).unwrap(), 123456789);
    }

    #[test]
    fn derives_documented_wallet_paths_deterministically() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let addresses = derive_addresses_from_mnemonic(mnemonic).unwrap();
        assert_eq!(addresses.len(), 6);
        assert_eq!(addresses.get("evm").unwrap(), "0x9858effd232b4033e47d90003d41ec34ecaeda94");
        assert_eq!(addresses.get("bitcoin").unwrap(), "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu");
        assert_eq!(addresses.get("zcash").unwrap(), "t1XVXWCvpMgBvUaed4XDqWtgQgJSu1Ghz7F");
        assert_eq!(addresses.get("solana").unwrap(), "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk");
        assert_eq!(addresses.get("filecoin").unwrap(), "f1fFXqnEMPFe1NoAajxRKukEBLwshG1LQQC");
        assert_eq!(addresses.get("injective").unwrap(), "inj1gsvdpdxec8hsu57lhxg5xem7refr233zkczfgv");
    }

    #[test]
    fn locked_session_does_not_expose_secrets() {
        let mut state = AppState::from_storage(PathBuf::from("/nonexistent/wallet.json"));
        let wallet = Wallet {
            name: "Secret Wallet".to_string(),
            mnemonic: "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            address: "0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string(),
            addresses: HashMap::new(),
            passphrase_hash: "deadbeef".to_string(),
            assets: vec![],
            activity: vec![],
        };
        state.wallet = Some(wallet);
        state.locked = true;
        state.stored_wallet = Some(StoredWalletMetadata {
            wallet_name: "Secret Wallet".to_string(),
        });
        let session = session_from_state(&state);
        assert_eq!(session.has_wallet, true);
        assert_eq!(session.locked, true);
        assert!(session.address.is_none());
        assert!(session.addresses.is_none());
        assert!(session.assets.is_empty());
        assert!(session.activity.is_empty());
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
            &[],
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
    fn encodes_erc20_transfer_abi() {
        let recipient = "0xdAC17F958D2ee523a2206206994597C13D831ec7";
        let amount: u128 = 1_000_000_000_000_000_000;
        let data = encode_erc20_transfer(recipient, amount).unwrap();
        assert!(!data.is_empty());
        assert_eq!(data.len(), 4 + 32 + 32); // selector + padded address + padded amount
        assert_eq!(&data[..4], &[0xa9, 0x05, 0x9c, 0xbb]); // keccak256("transfer(address,uint256)")[..4]
        let recip_bytes = hex::decode(recipient.trim_start_matches("0x")).unwrap();
        assert_eq!(&data[16..36], &recip_bytes[..]);
        // amount = 1 ETH = 0x0de0b6b3a7640000, padded to 32 bytes, so last byte is 0x00
        assert_eq!(data[data.len() - 1], 0x00);
    }

    #[test]
    fn signs_erc20_transfer() {
        use k256::ecdsa::SigningKey;
        let private_key = [0xabu8; 32];
        let signing_key = SigningKey::from_bytes((&private_key).into()).unwrap();
        let data = encode_erc20_transfer("0xdAC17F958D2ee523a2206206994597C13D831ec7", 1_000_000).unwrap();
        let result = sign_eip1559_transfer(
            &signing_key,
            1,
            0,
            1_000_000_000,
            1_000_000_000,
            50000,
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            0,
            &data,
        );
        assert!(result.is_ok());
        let (_raw, tx_hash, _raw_hex, r, s) = result.unwrap();
        assert!(tx_hash.starts_with("0x"));
        assert_eq!(tx_hash.len(), 66);
        assert_eq!(r.len(), 64);
        assert_eq!(s.len(), 64);
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
