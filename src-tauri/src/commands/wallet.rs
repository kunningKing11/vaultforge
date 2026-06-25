use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::Utc;
use std::{fs, sync::Mutex};
use tauri::State;
use zeroize::Zeroize;

use crate::activity::{activity, hash_secret};
use crate::derivation::{address_from_seed, derive_addresses_from_mnemonic, generate_mnemonic};
use crate::dto::{Wallet, WalletSession};
use crate::providers::fetch_portfolio_assets;
use crate::state::{AppState, StoredWalletMetadata, clear_secret_string, session_from_state};
use crate::storage::{
    decrypt_wallet, derive_storage_key, persist_state_wallet, read_stored_wallet,
};
use crate::validation::{clean_name, validate_passphrase};

#[tauri::command]
pub(crate) fn get_wallet(state: State<'_, Mutex<AppState>>) -> Result<WalletSession, String> {
    let state = state.lock().map_err(|_| "State lock failed")?;
    Ok(session_from_state(&state))
}

#[tauri::command]
pub(crate) async fn create_wallet(
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
pub(crate) async fn import_wallet(
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
pub(crate) async fn unlock_wallet(
    state: State<'_, Mutex<AppState>>,
    passphrase: String,
) -> Result<WalletSession, String> {
    let passphrase_hash = hash_secret(&passphrase);

    let (address, addresses, cached_assets) = {
        let mut state = state.lock().map_err(|_| "State lock failed")?;

        let in_memory = state.wallet.as_ref().map(|w| {
            (
                w.passphrase_hash.clone(),
                w.address.clone(),
                w.addresses.clone(),
                w.assets.clone(),
            )
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
    refresh_addresses
        .entry("evm".to_string())
        .or_insert(address);
    let fresh_assets = fetch_portfolio_assets(&refresh_addresses, &cached_assets).await;

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if let Some(wallet) = state.wallet.as_mut() {
        wallet.assets = fresh_assets;
    }
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
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
    state.locked = true;
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
    state.locked = false;
    Ok(session_from_state(&state))
}
