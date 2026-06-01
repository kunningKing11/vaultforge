use chrono::Utc;
use rand::{seq::SliceRandom, Rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use tauri::State;

#[derive(Clone)]
struct Wallet {
    name: String,
    address: String,
    passphrase_hash: String,
    assets: Vec<Asset>,
    activity: Vec<Activity>,
}

#[derive(Clone, Serialize)]
struct Asset {
    symbol: String,
    name: String,
    balance: f64,
    price_usd: f64,
    change_24h: f64,
    network: String,
}

#[derive(Clone, Serialize)]
struct Activity {
    id: String,
    kind: String,
    title: String,
    subtitle: String,
    amount: String,
    status: String,
    timestamp: String,
    hash: String,
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
}

struct AppState {
    wallet: Option<Wallet>,
    locked: bool,
    network: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            wallet: None,
            locked: false,
            network: "Ethereum".to_string(),
        }
    }
}

#[tauri::command]
fn get_wallet(state: State<'_, Mutex<AppState>>) -> Result<WalletSession, String> {
    let state = state.lock().map_err(|_| "State lock failed")?;
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
    state.wallet = Some(wallet);
    state.locked = false;
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
    state.wallet = Some(wallet);
    state.locked = false;
    Ok(session_from_state(&state))
}

#[tauri::command]
fn unlock_wallet(
    state: State<'_, Mutex<AppState>>,
    passphrase: String,
) -> Result<WalletSession, String> {
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    let wallet = state
        .wallet
        .as_ref()
        .ok_or_else(|| "No wallet exists yet".to_string())?;
    if wallet.passphrase_hash != hash_secret(&passphrase) {
        return Err("Invalid passphrase".to_string());
    }
    state.locked = false;
    Ok(session_from_state(&state))
}

#[tauri::command]
fn lock_wallet(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if state.wallet.is_some() {
        state.locked = true;
    }
    Ok(())
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

    let signed_at = Utc::now().to_rfc3339();
    let nonce = random_hex(6);
    let mut signed = SignedTransaction {
        from: wallet.address.clone(),
        to: to.trim().to_string(),
        symbol,
        amount,
        note: note.trim().to_string(),
        network: state.network.clone(),
        nonce,
        signed_at,
        payload_hash: String::new(),
        signature: String::new(),
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

    let asset = wallet
        .assets
        .iter_mut()
        .find(|asset| asset.symbol == signed.symbol)
        .ok_or_else(|| "Asset not found".to_string())?;
    if asset.balance < signed.amount {
        return Err(format!("Insufficient {} balance", signed.symbol));
    }

    asset.balance -= signed.amount;
    let memo = if signed.note.is_empty() {
        format!("Sent to {}", short_address(&signed.to))
    } else {
        signed.note.clone()
    };
    wallet.activity.insert(
        0,
        activity(
            "send",
            "Transfer sent",
            &memo,
            &format!("-{:.6} {}", signed.amount, signed.symbol),
        ),
    );
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
    if !to.starts_with("0x") || to.len() < 12 {
        return Err("Recipient must be a valid 0x address".to_string());
    }
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

    Ok(())
}

fn transaction_payload_hash(signed: &SignedTransaction) -> String {
    hash_secret(&format!(
        "from={};to={};symbol={};amount={:.12};note={};network={};nonce={};signed_at={}",
        signed.from,
        signed.to,
        signed.symbol,
        signed.amount,
        signed.note,
        signed.network,
        signed.nonce,
        signed.signed_at
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
    }
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
    let data: Vec<u8> = (0..bytes).map(|_| rng.gen()).collect();
    hex::encode(data)
}

fn short_address(address: &str) -> String {
    if address.len() <= 14 {
        return address.to_string();
    }
    format!("{}...{}", &address[..8], &address[address.len() - 6..])
}

fn main() {
    tauri::Builder::default()
        .manage(Mutex::new(AppState::default()))
        .invoke_handler(tauri::generate_handler![
            get_wallet,
            create_wallet,
            import_wallet,
            unlock_wallet,
            lock_wallet,
            sign_transaction,
            send_transaction,
            swap_tokens,
            set_network,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VaultForge Wallet");
}
