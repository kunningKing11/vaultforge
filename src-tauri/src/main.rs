use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use argon2::Argon2;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::Utc;
use rand::{seq::SliceRandom, Rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fs, path::PathBuf, sync::Mutex};
use tauri::{Manager, State};

#[derive(Clone, Deserialize, Serialize)]
struct Wallet {
    name: String,
    address: String,
    passphrase_hash: String,
    assets: Vec<Asset>,
    activity: Vec<Activity>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Asset {
    symbol: String,
    name: String,
    balance: f64,
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
    network: String,
    fiat_balance: f64,
    risk_score: u8,
    assets: Vec<Asset>,
    activity: Vec<Activity>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SignedTransaction {
    from: String,
    to: String,
    symbol: String,
    amount: f64,
    note: String,
    network: String,
    nonce: String,
    signed_at: String,
    payload_hash: String,
    signature: String,
    fee_amount: f64,
    fee_symbol: String,
    total_debit: f64,
    post_balance: f64,
    fiat_value: f64,
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
    network: String,
    stored_wallet: Option<StoredWalletMetadata>,
    encryption_key: Option<[u8; 32]>,
    storage_salt: Option<Vec<u8>>,
    storage_path: PathBuf,
}

#[derive(Clone)]
struct StoredWalletMetadata {
    wallet_name: String,
    address: String,
    network: String,
}

#[derive(Deserialize, Serialize)]
struct StoredWalletFile {
    version: u8,
    wallet_name: String,
    address: String,
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
                address: stored.address,
                network: stored.network,
            });
        let network = stored_wallet
            .as_ref()
            .map(|wallet| wallet.network.clone())
            .unwrap_or_else(|| "Ethereum".to_string());

        Self {
            wallet: None,
            locked: stored_wallet.is_some(),
            network,
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
fn create_wallet(
    state: State<'_, Mutex<AppState>>,
    name: String,
    passphrase: String,
) -> Result<WalletSession, String> {
    validate_passphrase(&passphrase)?;
    let mnemonic = generate_mnemonic();
    let wallet = Wallet {
        name: clean_name(name),
        address: address_from_seed(&mnemonic),
        passphrase_hash: hash_secret(&passphrase),
        assets: starter_assets("Ethereum"),
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
fn import_wallet(
    state: State<'_, Mutex<AppState>>,
    mnemonic: String,
    passphrase: String,
) -> Result<WalletSession, String> {
    let words = mnemonic.split_whitespace().count();
    if words != 12 && words != 24 {
        return Err("Recovery phrase must contain 12 or 24 words".to_string());
    }
    validate_passphrase(&passphrase)?;

    let wallet = Wallet {
        name: "Imported Wallet".to_string(),
        address: address_from_seed(&mnemonic),
        passphrase_hash: hash_secret(&passphrase),
        assets: starter_assets("Ethereum"),
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
fn unlock_wallet(
    state: State<'_, Mutex<AppState>>,
    passphrase: String,
) -> Result<WalletSession, String> {
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if let Some(wallet) = state.wallet.as_ref() {
        if wallet.passphrase_hash != hash_secret(&passphrase) {
            return Err("Invalid passphrase".to_string());
        }
        state.locked = false;
        return Ok(session_from_state(&state));
    }

    let stored = read_stored_wallet(&state.storage_path)?
        .ok_or_else(|| "No wallet exists yet".to_string())?;
    let wallet = decrypt_wallet(&stored, &passphrase)?;
    if wallet.passphrase_hash != hash_secret(&passphrase) {
        return Err("Invalid passphrase".to_string());
    }
    state.network = stored.network.clone();
    state.stored_wallet = Some(StoredWalletMetadata {
        wallet_name: stored.wallet_name,
        address: stored.address,
        network: stored.network,
    });
    let salt = BASE64
        .decode(stored.salt)
        .map_err(|_| "Stored wallet salt is invalid")?;
    let (key, salt) = derive_storage_key(&passphrase, Some(&salt))?;
    state.encryption_key = Some(key);
    state.storage_salt = Some(salt);
    state.wallet = Some(wallet);
    state.locked = false;
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
    state.network = "Ethereum".to_string();
    Ok(session_from_state(&state))
}

#[tauri::command]
fn sign_transaction(
    state: State<'_, Mutex<AppState>>,
    to: String,
    symbol: String,
    amount: f64,
    note: String,
) -> Result<SignedTransaction, String> {
    validate_unlocked(&state)?;

    let state = state.lock().map_err(|_| "State lock failed")?;
    let wallet = state
        .wallet
        .as_ref()
        .ok_or_else(|| "No wallet exists yet".to_string())?;
    validate_transfer(wallet, &to, &symbol, amount)?;
    let asset = wallet
        .assets
        .iter()
        .find(|asset| asset.symbol == symbol)
        .ok_or_else(|| "Asset not found".to_string())?;
    let fee_amount = transaction_fee(&symbol, amount);
    let total_debit = amount + fee_amount;
    if asset.balance < total_debit {
        return Err(format!(
            "Insufficient {} balance for amount plus fee",
            symbol
        ));
    }

    let signed_at = Utc::now().to_rfc3339();
    let nonce = random_hex(6);
    let mut signed = SignedTransaction {
        from: wallet.address.clone(),
        to: to.trim().to_string(),
        symbol: symbol.clone(),
        amount,
        note: note.trim().to_string(),
        network: state.network.clone(),
        nonce,
        signed_at,
        payload_hash: String::new(),
        signature: String::new(),
        fee_amount,
        fee_symbol: symbol.clone(),
        total_debit,
        post_balance: asset.balance - total_debit,
        fiat_value: amount * asset.price_usd,
    };
    signed.payload_hash = transaction_payload_hash(&signed);
    signed.signature = transaction_signature(wallet, &signed.payload_hash);

    Ok(signed)
}

#[tauri::command]
fn send_transaction(
    state: State<'_, Mutex<AppState>>,
    signed: SignedTransaction,
) -> Result<WalletSession, String> {
    validate_unlocked(&state)?;

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    let active_network = state.network.clone();
    let wallet = state
        .wallet
        .as_mut()
        .ok_or_else(|| "No wallet exists yet".to_string())?;
    validate_transfer(wallet, &signed.to, &signed.symbol, signed.amount)?;
    if signed.from != wallet.address.as_str() {
        return Err("Signed transaction does not match this wallet".to_string());
    }
    if signed.network != active_network {
        return Err("Signed transaction network no longer matches the active network".to_string());
    }
    let expected_hash = transaction_payload_hash(&signed);
    if signed.payload_hash != expected_hash {
        return Err("Signed transaction payload was modified".to_string());
    }
    if signed.signature != transaction_signature(wallet, &signed.payload_hash) {
        return Err("Invalid transaction signature".to_string());
    }
    if signed.fee_amount != transaction_fee(&signed.symbol, signed.amount)
        || signed.fee_symbol != signed.symbol
        || (signed.total_debit - (signed.amount + signed.fee_amount)).abs() > f64::EPSILON
    {
        return Err("Signed transaction fee details are invalid".to_string());
    }

    let asset = wallet
        .assets
        .iter_mut()
        .find(|asset| asset.symbol == signed.symbol)
        .ok_or_else(|| "Asset not found".to_string())?;
    if asset.balance < signed.total_debit {
        return Err(format!(
            "Insufficient {} balance for amount plus fee",
            signed.symbol
        ));
    }

    asset.balance -= signed.total_debit;
    let memo = if signed.note.is_empty() {
        format!("Sent to {}", short_address(&signed.to))
    } else {
        signed.note.clone()
    };
    wallet.activity.insert(0, send_activity(&signed, &memo));
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

#[tauri::command(rename_all = "camelCase")]
fn swap_tokens(
    state: State<'_, Mutex<AppState>>,
    from_symbol: String,
    to_symbol: String,
    amount: f64,
) -> Result<WalletSession, String> {
    validate_unlocked(&state)?;
    if from_symbol == to_symbol {
        return Err("Choose two different assets".to_string());
    }
    if amount <= 0.0 || !amount.is_finite() {
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

    if wallet.assets[from_index].balance < amount {
        return Err(format!("Insufficient {} balance", from_symbol));
    }

    let source_value = amount * wallet.assets[from_index].price_usd;
    let received = (source_value / wallet.assets[to_index].price_usd) * 0.995;
    wallet.assets[from_index].balance -= amount;
    wallet.assets[to_index].balance += received;
    wallet.activity.insert(
        0,
        activity(
            "swap",
            "Swap executed",
            &format!("{} to {} with 0.5% route fee", from_symbol, to_symbol),
            &format!("{amount:.6} {from_symbol} -> {received:.6} {to_symbol}"),
        ),
    );
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

#[tauri::command]
fn set_network(
    state: State<'_, Mutex<AppState>>,
    network: String,
) -> Result<WalletSession, String> {
    let supported = ["Ethereum", "Polygon", "Arbitrum", "Base", "Optimism"];
    if !supported.contains(&network.as_str()) {
        return Err("Unsupported network".to_string());
    }

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    state.network = network.clone();
    if let Some(wallet) = state.wallet.as_mut() {
        for asset in &mut wallet.assets {
            asset.network = network.clone();
        }
        wallet.activity.insert(
            0,
            activity("network", "Network changed", &network, "Updated"),
        );
    }
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

fn validate_transfer(wallet: &Wallet, to: &str, symbol: &str, amount: f64) -> Result<(), String> {
    let to = to.trim();
    if amount <= 0.0 || !amount.is_finite() {
        return Err("Amount must be greater than zero".to_string());
    }

    let asset = wallet
        .assets
        .iter()
        .find(|asset| asset.symbol == symbol)
        .ok_or_else(|| "Asset not found".to_string())?;
    if asset.balance < amount {
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

fn transaction_fee(symbol: &str, amount: f64) -> f64 {
    match symbol {
        "BTC" => 0.00001,
        "SOL" => 0.000005,
        "USDC" => (amount * 0.001).max(0.25),
        _ => 0.00042,
    }
}

fn transaction_payload_hash(signed: &SignedTransaction) -> String {
    hash_secret(&format!(
        "from={};to={};symbol={};amount={:.12};note={};network={};nonce={};signed_at={};fee_amount={:.12};fee_symbol={};total_debit={:.12};post_balance={:.12};fiat_value={:.12}",
        signed.from,
        signed.to,
        signed.symbol,
        signed.amount,
        signed.note,
        signed.network,
        signed.nonce,
        signed.signed_at,
        signed.fee_amount,
        signed.fee_symbol,
        signed.total_debit,
        signed.post_balance,
        signed.fiat_value
    ))
}

fn transaction_signature(wallet: &Wallet, payload_hash: &str) -> String {
    format!(
        "0x{}",
        hash_secret(&format!(
            "{}:{}:{}",
            wallet.address, wallet.passphrase_hash, payload_hash
        ))
    )
}

fn session_from_state(state: &AppState) -> WalletSession {
    let Some(wallet) = state.wallet.as_ref() else {
        if let Some(stored_wallet) = state.stored_wallet.as_ref() {
            return WalletSession {
                has_wallet: true,
                locked: true,
                wallet_name: Some(stored_wallet.wallet_name.clone()),
                address: Some(stored_wallet.address.clone()),
                network: stored_wallet.network.clone(),
                fiat_balance: 0.0,
                risk_score: 92,
                assets: vec![],
                activity: vec![],
            };
        }

        return WalletSession {
            has_wallet: false,
            locked: false,
            wallet_name: None,
            address: None,
            network: state.network.clone(),
            fiat_balance: 0.0,
            risk_score: 0,
            assets: vec![],
            activity: vec![],
        };
    };

    if state.locked {
        return WalletSession {
            has_wallet: true,
            locked: true,
            wallet_name: Some(wallet.name.clone()),
            address: Some(wallet.address.clone()),
            network: state.network.clone(),
            fiat_balance: 0.0,
            risk_score: 92,
            assets: vec![],
            activity: vec![],
        };
    }

    let fiat_balance = wallet
        .assets
        .iter()
        .map(|asset| asset.balance * asset.price_usd)
        .sum();

    WalletSession {
        has_wallet: true,
        locked: false,
        wallet_name: Some(wallet.name.clone()),
        address: Some(wallet.address.clone()),
        network: state.network.clone(),
        fiat_balance,
        risk_score: 92,
        assets: wallet.assets.clone(),
        activity: wallet.activity.clone(),
    }
}

fn starter_assets(network: &str) -> Vec<Asset> {
    vec![
        Asset {
            symbol: "ETH".to_string(),
            name: "Ethereum".to_string(),
            balance: 2.4821,
            price_usd: 3480.62,
            change_24h: 2.84,
            network: network.to_string(),
        },
        Asset {
            symbol: "BTC".to_string(),
            name: "Bitcoin".to_string(),
            balance: 0.1842,
            price_usd: 102_240.12,
            change_24h: -0.62,
            network: network.to_string(),
        },
        Asset {
            symbol: "SOL".to_string(),
            name: "Solana".to_string(),
            balance: 82.45,
            price_usd: 184.33,
            change_24h: 5.18,
            network: network.to_string(),
        },
        Asset {
            symbol: "USDC".to_string(),
            name: "USD Coin".to_string(),
            balance: 8_420.0,
            price_usd: 1.0,
            change_24h: 0.01,
            network: network.to_string(),
        },
    ]
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

fn send_activity(signed: &SignedTransaction, subtitle: &str) -> Activity {
    Activity {
        id: random_hex(8),
        kind: "send".to_string(),
        title: "Transfer sent".to_string(),
        subtitle: subtitle.to_string(),
        amount: format!("-{:.6} {}", signed.amount, signed.symbol),
        status: "confirmed".to_string(),
        timestamp: Utc::now().to_rfc3339(),
        hash: format!("0x{}", &signed.payload_hash[..32]),
        from: Some(signed.from.clone()),
        to: Some(signed.to.clone()),
        network: Some(signed.network.clone()),
        payload_hash: Some(signed.payload_hash.clone()),
        signature: Some(signed.signature.clone()),
        fee: Some(format!("{:.6} {}", signed.fee_amount, signed.fee_symbol)),
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

    let stored = encrypt_wallet(wallet, &state.network, &key, &salt)?;
    if let Some(parent) = state.storage_path.parent() {
        fs::create_dir_all(parent).map_err(|_| "Failed to create wallet storage directory")?;
    }
    let contents = serde_json::to_string_pretty(&stored).map_err(|_| "Failed to encode wallet")?;
    fs::write(&state.storage_path, contents).map_err(|_| "Failed to save wallet")?;
    state.stored_wallet = Some(StoredWalletMetadata {
        wallet_name: stored.wallet_name,
        address: stored.address,
        network: stored.network,
    });
    Ok(())
}

fn encrypt_wallet(
    wallet: &Wallet,
    network: &str,
    key: &[u8; 32],
    salt: &[u8],
) -> Result<StoredWalletFile, String> {
let nonce_bytes: Vec<u8> = (0..12).map(|_| rand::thread_rng().r#gen()).collect();
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "Failed to initialize encryption")?;
    let plaintext = serde_json::to_vec(wallet).map_err(|_| "Failed to encode wallet")?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
        .map_err(|_| "Failed to encrypt wallet")?;

    Ok(StoredWalletFile {
        version: 1,
        wallet_name: wallet.name.clone(),
        address: wallet.address.clone(),
        network: network.to_string(),
        salt: BASE64.encode(salt),
        nonce: BASE64.encode(nonce_bytes),
        ciphertext: BASE64.encode(ciphertext),
    })
}

fn decrypt_wallet(stored: &StoredWalletFile, passphrase: &str) -> Result<Wallet, String> {
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
    serde_json::from_slice(&plaintext).map_err(|_| "Stored wallet contents are invalid".to_string())
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

fn generate_mnemonic() -> String {
    const WORDS: &[&str] = &[
        "amber", "anchor", "atlas", "binary", "cactus", "cannon", "carbon", "copper", "cosmic",
        "delta", "ember", "fabric", "galaxy", "harbor", "island", "jungle", "kernel", "ladder",
        "magnet", "matrix", "nebula", "orbit", "pioneer", "quantum", "rocket", "saddle", "signal",
        "silver", "summit", "token", "velvet", "voyage", "window", "yellow", "zenith", "zero",
    ];
    let mut rng = rand::thread_rng();
    (0..12)
        .map(|_| WORDS.choose(&mut rng).unwrap_or(&"vault").to_string())
        .collect::<Vec<_>>()
        .join(" ")
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
    fn calculates_simulated_fees() {
        assert_eq!(transaction_fee("BTC", 1.0), 0.00001);
        assert_eq!(transaction_fee("SOL", 1.0), 0.000005);
        assert_eq!(transaction_fee("ETH", 1.0), 0.00042);
        assert_eq!(transaction_fee("USDC", 100.0), 0.25);
        assert_eq!(transaction_fee("USDC", 1_000.0), 1.0);
    }

    #[test]
    fn payload_hash_changes_when_details_change() {
        let mut signed = SignedTransaction {
            from: "0xfrom".to_string(),
            to: "0xto".to_string(),
            symbol: "ETH".to_string(),
            amount: 1.0,
            note: "memo".to_string(),
            network: "Ethereum".to_string(),
            nonce: "abc".to_string(),
            signed_at: "2026-01-01T00:00:00Z".to_string(),
            payload_hash: String::new(),
            signature: String::new(),
            fee_amount: 0.00042,
            fee_symbol: "ETH".to_string(),
            total_debit: 1.00042,
            post_balance: 2.0,
            fiat_value: 3480.62,
        };
        let first = transaction_payload_hash(&signed);
        signed.amount = 2.0;
        assert_ne!(first, transaction_payload_hash(&signed));
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
            address: address_from_seed("test seed"),
            passphrase_hash: hash_secret(passphrase),
            assets: starter_assets("Ethereum"),
            activity: vec![activity("system", "Created", "Local", "1")],
        };
        let (key, salt) = derive_storage_key(passphrase, None).unwrap();
        let stored = encrypt_wallet(&wallet, "Ethereum", &key, &salt).unwrap();
        assert!(!stored.ciphertext.contains(&wallet.address));

        let decrypted = decrypt_wallet(&stored, passphrase).unwrap();
        assert_eq!(decrypted.name, wallet.name);
        assert_eq!(decrypted.address, wallet.address);
        assert_eq!(decrypted.assets.len(), wallet.assets.len());
    }

    #[test]
    fn rejects_wrong_storage_passphrase() {
        let wallet = Wallet {
            name: "Test Wallet".to_string(),
            address: address_from_seed("test seed"),
            passphrase_hash: hash_secret("right passphrase 42!"),
            assets: starter_assets("Ethereum"),
            activity: vec![],
        };
        let (key, salt) = derive_storage_key("right passphrase 42!", None).unwrap();
        let stored = encrypt_wallet(&wallet, "Ethereum", &key, &salt).unwrap();

        assert!(decrypt_wallet(&stored, "wrong passphrase 42!").is_err());
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
            set_network,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VaultForge Wallet");
}
