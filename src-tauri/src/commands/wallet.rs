use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::Utc;
use std::{collections::HashMap, fs, sync::Mutex};
use tauri::State;
use zeroize::Zeroize;

use crate::activity::{activity, hash_secret};
use crate::commands::market::refresh_asset_prices;
use crate::derivation::{
    address_from_seed, address_key_for_network, derive_addresses_from_mnemonic_filtered,
    generate_mnemonic, validate_recovery_phrase_word_count,
};
use crate::dto::{Wallet, WalletSession};
use crate::providers::fetch_portfolio_assets;
use crate::state::{AppState, StoredWalletMetadata, clear_secret_string, session_from_state};
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
    name: String,
    wallet_password: String,
    enabled_networks: Vec<String>,
    auto_lock_timeout_secs: Option<u64>,
    mnemonic: Option<String>,
) -> Result<WalletSession, String> {
    validate_wallet_password(&wallet_password)?;
    let mnemonic = match mnemonic {
        Some(m) if !m.trim().is_empty() => m.trim().to_string(),
        _ => generate_mnemonic(12)?,
    };
    let network_refs: Vec<&str> = enabled_networks.iter().map(|s| s.as_str()).collect();
    let addresses = derive_addresses_from_mnemonic_filtered(&mnemonic, &network_refs)?;
    let primary_address = addresses
        .get("evm")
        .cloned()
        .unwrap_or_else(|| address_from_seed(&mnemonic));

    let mut assets = fetch_portfolio_assets(&addresses, &[]).await;
    let _ = refresh_asset_prices(&mut assets).await;

    let wallet = Wallet {
        name: clean_name(name),
        mnemonic,
        created_at: Utc::now().to_rfc3339(),
        address: primary_address,
        addresses,
        wallet_password_hash: hash_secret(&wallet_password),
        assets,
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
    state.wallet = Some(wallet);
    state.locked = false;
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

#[tauri::command]
pub(crate) async fn import_wallet(
    state: State<'_, Mutex<AppState>>,
    name: Option<String>,
    mnemonic: String,
    wallet_password: String,
    enabled_networks: Vec<String>,
    auto_lock_timeout_secs: Option<u64>,
) -> Result<WalletSession, String> {
    let mnemonic = mnemonic.trim().to_string();
    validate_recovery_phrase_word_count(&mnemonic)?;
    validate_wallet_password(&wallet_password)?;

    let network_refs: Vec<&str> = enabled_networks.iter().map(|s| s.as_str()).collect();
    let addresses = derive_addresses_from_mnemonic_filtered(&mnemonic, &network_refs)?;
    let primary_address = addresses
        .get("evm")
        .cloned()
        .unwrap_or_else(|| address_from_seed(&mnemonic));

    let mut assets = fetch_portfolio_assets(&addresses, &[]).await;
    let _ = refresh_asset_prices(&mut assets).await;

    let wallet = Wallet {
        name: clean_name(name.unwrap_or_else(|| "Imported Wallet".to_string())),
        mnemonic,
        created_at: Utc::now().to_rfc3339(),
        address: primary_address,
        addresses,
        wallet_password_hash: hash_secret(&wallet_password),
        assets,
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
    state.wallet = Some(wallet);
    state.locked = false;
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

fn enabled_address_subset(
    addresses: &HashMap<String, String>,
    enabled_networks: &[String],
) -> HashMap<String, String> {
    let mut filtered = HashMap::new();
    for network_id in enabled_networks {
        let Some(family) = address_key_for_network(network_id.as_str()) else {
            continue;
        };
        if let Some(address) = addresses.get(family) {
            filtered.insert(family.to_string(), address.clone());
        }
    }
    filtered
}

#[tauri::command]
pub(crate) async fn unlock_wallet(
    state: State<'_, Mutex<AppState>>,
    wallet_password: String,
) -> Result<WalletSession, String> {
    let wallet_password_hash = hash_secret(&wallet_password);

    let (address, addresses, cached_assets, enabled_networks) = {
        let mut state = state.lock().map_err(|_| "State lock failed")?;

        let in_memory = state.wallet.as_ref().map(|w| {
            (
                w.wallet_password_hash.clone(),
                w.address.clone(),
                w.addresses.clone(),
                w.assets.clone(),
                w.enabled_networks.clone(),
            )
        });

        if let Some((stored_hash, addr, addresses, assets, enabled_networks)) = in_memory {
            if stored_hash != wallet_password_hash {
                return Err("Invalid wallet password".to_string());
            }
            state.locked = false;
            (addr, addresses, assets, enabled_networks)
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
            let salt = BASE64
                .decode(stored.salt)
                .map_err(|_| "Stored wallet salt is invalid")?;
            let (key, salt) = derive_storage_key(&wallet_password, Some(&salt))?;
            state.encryption_key = Some(key);
            state.storage_salt = Some(salt);
            let address = wallet.address.clone();
            let addresses = wallet.addresses.clone();
            let assets = wallet.assets.clone();
            let enabled_networks = wallet.enabled_networks.clone();
            state.wallet = Some(wallet);
            state.locked = false;
            (address, addresses, assets, enabled_networks)
        }
    };

    let mut refresh_addresses = enabled_address_subset(&addresses, &enabled_networks);
    if refresh_addresses.is_empty() {
        for network_id in &enabled_networks {
            let Some(family) = address_key_for_network(network_id.as_str()) else {
                continue;
            };
            if let Some(address_value) = addresses.get(family) {
                refresh_addresses.insert(family.to_string(), address_value.clone());
                break;
            }
        }
    }
    if !refresh_addresses.contains_key("evm")
        && enabled_networks
            .iter()
            .any(|n| address_key_for_network(n.as_str()) == Some("evm"))
    {
        if let Some(address_value) = addresses.get("evm") {
            refresh_addresses.insert("evm".to_string(), address_value.clone());
        }
    }
    let primary_address = refresh_addresses
        .values()
        .next()
        .cloned()
        .unwrap_or_else(|| address.clone());
    let _ = primary_address;
    let mut fresh_assets = fetch_portfolio_assets(&refresh_addresses, &cached_assets).await;
    let _ = refresh_asset_prices(&mut fresh_assets).await;

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
