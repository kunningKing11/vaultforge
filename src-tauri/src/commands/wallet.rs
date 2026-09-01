use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::Utc;
use std::{fs, sync::Mutex};
use tauri::State;
use zeroize::Zeroize;

use crate::activity::{activity, hash_secret};
use crate::commands::market::refresh_wallet_portfolio;
use crate::derivation::{
    BitcoinAccount, derive_addresses_from_mnemonic_filtered, generate_mnemonic,
    validate_recovery_phrase_word_count,
};
use crate::dto::{FiatCurrency, Wallet, WalletRefreshResult, WalletSession};
use crate::providers::bitcoin::BitcoinAccountSnapshot;
use crate::providers::http::ProviderClients;
use crate::state::{
    AppState, StoredWalletMetadata, clear_secret_string, refresh_result_from_state,
    session_from_state,
};
use crate::storage::{
    decrypt_wallet, derive_storage_key, persist_state_wallet, read_stored_wallet,
};
use crate::validation::{clean_name, validate_wallet_password};

pub(crate) fn refresh_filecoin_address(wallet: &mut Wallet) -> Result<(), String> {
    let addresses = derive_addresses_from_mnemonic_filtered(&wallet.mnemonic, &["filecoin"])?;
    let filecoin_address = addresses
        .get("filecoin")
        .cloned()
        .ok_or_else(|| "Filecoin address derivation is unavailable".to_string())?;
    wallet
        .addresses
        .insert("filecoin".to_string(), filecoin_address);
    Ok(())
}

fn initialize_bitcoin_account(
    mnemonic: &str,
    enabled_networks: &[String],
) -> Result<Option<BitcoinAccountSnapshot>, String> {
    if !enabled_networks.iter().any(|network| network == "bitcoin") {
        return Ok(None);
    }
    Ok(Some(BitcoinAccountSnapshot::new(
        BitcoinAccount::from_mnemonic(mnemonic)?,
    )))
}

#[tauri::command]
pub(crate) fn get_wallet(state: State<'_, Mutex<AppState>>) -> Result<WalletSession, String> {
    let state = state.lock().map_err(|_| "State lock failed")?;
    Ok(session_from_state(&state))
}

#[tauri::command]
pub(crate) fn generate_mnemonic_cmd(word_count: Option<u32>) -> Result<String, String> {
    generate_mnemonic(word_count.unwrap_or(24))
}

#[tauri::command]
pub(crate) async fn create_wallet(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    name: String,
    wallet_password: String,
    enabled_networks: Vec<String>,
    auto_lock_timeout_secs: Option<u64>,
    mnemonic: Option<String>,
) -> Result<WalletRefreshResult, String> {
    validate_wallet_password(&wallet_password)?;
    let mnemonic = match mnemonic {
        Some(m) if !m.trim().is_empty() => m.trim().to_string(),
        _ => generate_mnemonic(12)?,
    };
    let network_refs: Vec<&str> = enabled_networks.iter().map(|s| s.as_str()).collect();
    let addresses = derive_addresses_from_mnemonic_filtered(&mnemonic, &network_refs)?;
    let bitcoin_account = initialize_bitcoin_account(&mnemonic, &enabled_networks)?;
    let refreshed = refresh_wallet_portfolio(
        clients.http(),
        &addresses,
        &[],
        &enabled_networks,
        bitcoin_account.as_ref(),
        FiatCurrency::Usd,
        1.0,
    )
    .await;

    let wallet = Wallet {
        name: clean_name(name),
        mnemonic,
        created_at: Utc::now().to_rfc3339(),
        addresses,
        wallet_password_hash: hash_secret(&wallet_password),
        fiat_currency: FiatCurrency::Usd,
        usd_exchange_rate: refreshed.usd_exchange_rate,
        assets: refreshed.assets,
        activity: vec![activity(
            "system",
            "Wallet created",
            "Recovery phrase generated locally",
            "12 words",
        )],
        enabled_networks,
        auto_lock_timeout_secs,
    };

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    let (key, salt) = derive_storage_key(&wallet_password, None)?;
    state.encryption_key = Some(key);
    state.storage_salt = Some(salt);
    state.bitcoin_account = refreshed.bitcoin_account;
    state.wallet = Some(wallet);
    state.locked = false;
    state.advance_wallet_generation();
    persist_state_wallet(&mut state)?;
    Ok(refresh_result_from_state(&state, refreshed.warnings))
}

#[tauri::command]
pub(crate) async fn import_wallet(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    name: Option<String>,
    mnemonic: String,
    wallet_password: String,
    enabled_networks: Vec<String>,
    auto_lock_timeout_secs: Option<u64>,
) -> Result<WalletRefreshResult, String> {
    let mnemonic = mnemonic.trim().to_string();
    validate_recovery_phrase_word_count(&mnemonic)?;
    validate_wallet_password(&wallet_password)?;

    let network_refs: Vec<&str> = enabled_networks.iter().map(|s| s.as_str()).collect();
    let addresses = derive_addresses_from_mnemonic_filtered(&mnemonic, &network_refs)?;
    let bitcoin_account = initialize_bitcoin_account(&mnemonic, &enabled_networks)?;
    let refreshed = refresh_wallet_portfolio(
        clients.http(),
        &addresses,
        &[],
        &enabled_networks,
        bitcoin_account.as_ref(),
        FiatCurrency::Usd,
        1.0,
    )
    .await;

    let wallet = Wallet {
        name: clean_name(name.unwrap_or_else(|| "Imported Wallet".to_string())),
        mnemonic,
        created_at: Utc::now().to_rfc3339(),
        addresses,
        wallet_password_hash: hash_secret(&wallet_password),
        fiat_currency: FiatCurrency::Usd,
        usd_exchange_rate: refreshed.usd_exchange_rate,
        assets: refreshed.assets,
        activity: vec![activity(
            "import",
            "Wallet imported",
            "Recovery phrase verified locally",
            "Imported",
        )],
        enabled_networks,
        auto_lock_timeout_secs,
    };

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    let (key, salt) = derive_storage_key(&wallet_password, None)?;
    state.encryption_key = Some(key);
    state.storage_salt = Some(salt);
    state.bitcoin_account = refreshed.bitcoin_account;
    state.wallet = Some(wallet);
    state.locked = false;
    state.advance_wallet_generation();
    persist_state_wallet(&mut state)?;
    Ok(refresh_result_from_state(&state, refreshed.warnings))
}

#[tauri::command]
pub(crate) async fn unlock_wallet(
    state: State<'_, Mutex<AppState>>,
    wallet_password: String,
) -> Result<WalletRefreshResult, String> {
    let wallet_password_hash = hash_secret(&wallet_password);

    let bitcoin_account = {
        let mut state = state.lock().map_err(|_| "State lock failed")?;
        let cached_bitcoin_account = state.bitcoin_account.clone();

        let in_memory = state.wallet.as_ref().map(|wallet| {
            (
                wallet.wallet_password_hash.clone(),
                wallet.enabled_networks.clone(),
                wallet.mnemonic.clone(),
            )
        });

        if let Some((stored_hash, enabled_networks, mut mnemonic)) = in_memory {
            if stored_hash != wallet_password_hash {
                clear_secret_string(&mut mnemonic);
                return Err("Invalid wallet password".to_string());
            }
            let bitcoin_account = if enabled_networks.iter().any(|network| network == "bitcoin") {
                match cached_bitcoin_account {
                    Some(account) => Ok(Some(account)),
                    None => initialize_bitcoin_account(&mnemonic, &enabled_networks),
                }
            } else {
                Ok(None)
            };
            clear_secret_string(&mut mnemonic);
            let bitcoin_account = bitcoin_account?;
            state.locked = false;
            bitcoin_account
        } else {
            let stored = read_stored_wallet(&state.storage_path)?
                .ok_or_else(|| "No wallet exists yet".to_string())?;
            let mut wallet = decrypt_wallet(&stored, &wallet_password)?;
            if wallet.wallet_password_hash != wallet_password_hash {
                return Err("Invalid wallet password".to_string());
            }
            refresh_filecoin_address(&mut wallet)?;
            state.stored_wallet = Some(StoredWalletMetadata {
                wallet_name: stored.wallet_name,
            });
            let salt = BASE64 // check General Purpose Engine for decoding base64 for cryptographic purposes
                .decode(stored.salt)
                .map_err(|_| "Stored wallet salt is invalid")?;
            let (key, salt) = derive_storage_key(&wallet_password, Some(&salt))?;
            state.encryption_key = Some(key);
            state.storage_salt = Some(salt);
            let enabled_networks = wallet.enabled_networks.clone();
            let bitcoin_account = initialize_bitcoin_account(&wallet.mnemonic, &enabled_networks)?;
            state.wallet = Some(wallet);
            state.locked = false;
            bitcoin_account
        }
    };

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    state.bitcoin_account = bitcoin_account;
    state.advance_wallet_generation();
    Ok(refresh_result_from_state(&state, vec![]))
}

#[tauri::command]
pub(crate) fn lock_wallet(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
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
    state.bitcoin_account = None;
    state.locked = true;
    state.advance_wallet_generation();
    Ok(())
}

#[tauri::command]
pub(crate) fn clear_wallet(state: State<'_, Mutex<AppState>>) -> Result<WalletSession, String> {
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
    state.bitcoin_account = None;
    state.locked = false;
    state.advance_wallet_generation();
    Ok(session_from_state(&state))
}

#[cfg(test)]
#[path = "../tests/commands/wallet.rs"]
mod tests;
