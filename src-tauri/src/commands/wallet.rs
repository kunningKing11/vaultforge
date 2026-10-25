use chrono::Utc;
use std::{fs, sync::Mutex};
use tauri::State;
use zeroize::{Zeroize, Zeroizing};

use crate::activity::{activity, hash_secret};
use crate::commands::market::refresh_wallet_portfolio;
use crate::derivation::{
    BitcoinAccount, derive_addresses_from_mnemonic_filtered, generate_mnemonic,
    validate_recovery_phrase_word_count,
};
use crate::dto::{
    FiatCurrency, Wallet, WalletRefreshResult, WalletSession, default_hide_below_usd,
};
use crate::operations::Operation;
use crate::operations::reconcile_operation;
use crate::providers::bitcoin::BitcoinAccountSnapshot;
use crate::providers::http::ProviderClients;
use crate::providers::prices::fetch_usd_exchange_rate;
use crate::state::{
    AppState, StoredWalletMetadata, clear_secret_string, refresh_result_from_state,
    session_from_state,
};
use crate::storage::{
    decrypt_wallet, derive_storage_key, persist_state_wallet, read_stored_wallet,
};
use crate::validation::{clean_name, validate_wallet_password};

/// Derives and inserts a missing Filecoin address into an older wallet.
pub(crate) fn refresh_filecoin_address(wallet: &mut Wallet) -> Result<(), String> {
    let addresses = derive_addresses_from_mnemonic_filtered(
        &wallet.mnemonic,
        &wallet.bip39_passphrase,
        &["filecoin"],
    )?;
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
    bip39_passphrase: &str,
    enabled_networks: &[String],
) -> Result<Option<BitcoinAccountSnapshot>, String> {
    if !enabled_networks.iter().any(|network| network == "bitcoin") {
        return Ok(None);
    }
    Ok(Some(BitcoinAccountSnapshot::new(
        BitcoinAccount::from_mnemonic(mnemonic, bip39_passphrase)?,
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

fn apply_wallet_settings(
    wallet: &mut Wallet,
    name: String,
    fiat_currency: FiatCurrency,
    usd_exchange_rate: f64,
    auto_lock_timeout_secs: Option<u64>,
    use_crypto_symbols: bool,
    hide_below_usd: f64,
    group_assets_by_chain: bool,
) {
    wallet.name = clean_name(name);
    wallet.fiat_currency = fiat_currency;
    wallet.usd_exchange_rate = usd_exchange_rate;
    wallet.auto_lock_timeout_secs = auto_lock_timeout_secs;
    wallet.use_crypto_symbols = use_crypto_symbols;
    wallet.hide_below_usd = hide_below_usd;
    wallet.group_assets_by_chain = group_assets_by_chain;
}

fn validate_hide_below_usd(hide_below_usd: f64) -> Result<(), String> {
    if !hide_below_usd.is_finite() || hide_below_usd < 0.0 {
        return Err("Hide-below amount must be a non-negative dollar amount".to_string());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn update_wallet_settings(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    name: String,
    fiat_currency: FiatCurrency,
    auto_lock_timeout_secs: Option<u64>,
    use_crypto_symbols: bool,
    hide_below_usd: f64,
    group_assets_by_chain: bool,
) -> Result<WalletSession, String> {
    let (wallet_generation, current_currency, current_exchange_rate) = {
        let state = state.lock().map_err(|_| "State lock failed")?;
        if state.locked {
            return Err("Wallet is locked".to_string());
        }
        let wallet = state
            .wallet
            .as_ref()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        (
            state.wallet_generation,
            wallet.fiat_currency,
            wallet.usd_exchange_rate,
        )
    };

    let usd_exchange_rate = if fiat_currency == current_currency {
        current_exchange_rate
    } else {
        fetch_usd_exchange_rate(clients.http(), fiat_currency).await?
    };

    validate_hide_below_usd(hide_below_usd)?;
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if !state.can_commit_refresh(wallet_generation) {
        return Err("Wallet changed while updating settings".to_string());
    }
    let wallet = state
        .wallet
        .as_mut()
        .ok_or_else(|| "No wallet exists yet".to_string())?;
    apply_wallet_settings(
        wallet,
        name,
        fiat_currency,
        usd_exchange_rate,
        auto_lock_timeout_secs,
        use_crypto_symbols,
        hide_below_usd,
        group_assets_by_chain,
    );
    state.advance_wallet_generation();
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

#[tauri::command]
pub(crate) async fn create_wallet(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    name: String,
    wallet_password: String,
    fiat_currency: FiatCurrency,
    enabled_networks: Vec<String>,
    auto_lock_timeout_secs: Option<u64>,
    use_crypto_symbols: bool,
    mnemonic: Option<String>,
    bip39_passphrase: Option<String>,
) -> Result<WalletRefreshResult, String> {
    validate_wallet_password(&wallet_password)?;
    let mut bip39_passphrase = Zeroizing::new(bip39_passphrase.unwrap_or_default());
    let mnemonic = match mnemonic {
        Some(m) if !m.trim().is_empty() => m.trim().to_string(),
        _ => generate_mnemonic(12)?,
    };
    let network_refs: Vec<&str> = enabled_networks.iter().map(|s| s.as_str()).collect();
    let addresses =
        derive_addresses_from_mnemonic_filtered(&mnemonic, &bip39_passphrase, &network_refs)?;
    let bitcoin_account =
        initialize_bitcoin_account(&mnemonic, &bip39_passphrase, &enabled_networks)?;
    let refreshed = refresh_wallet_portfolio(
        clients.http(),
        &addresses,
        &[],
        &enabled_networks,
        bitcoin_account.as_ref(),
        fiat_currency,
        1.0,
    )
    .await;

    let wallet = Wallet {
        name: clean_name(name),
        mnemonic,
        bip39_passphrase: std::mem::take(&mut *bip39_passphrase),
        created_at: Utc::now().to_rfc3339(),
        addresses,
        wallet_password_hash: hash_secret(&wallet_password),
        fiat_currency,
        usd_exchange_rate: refreshed.usd_exchange_rate,
        assets: refreshed.assets,
        activity: vec![activity(
            "system",
            "Wallet created",
            "Recovery phrase generated locally",
            "12 words",
        )],
        operations: Vec::new(),
        enabled_networks,
        auto_lock_timeout_secs,
        use_crypto_symbols,
        hide_below_usd: default_hide_below_usd(),
        group_assets_by_chain: true,
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
    fiat_currency: FiatCurrency,
    enabled_networks: Vec<String>,
    auto_lock_timeout_secs: Option<u64>,
    use_crypto_symbols: bool,
    bip39_passphrase: Option<String>,
) -> Result<WalletRefreshResult, String> {
    let mut bip39_passphrase = Zeroizing::new(bip39_passphrase.unwrap_or_default());
    let mnemonic = mnemonic.trim().to_string();
    validate_recovery_phrase_word_count(&mnemonic)?;
    validate_wallet_password(&wallet_password)?;

    let network_refs: Vec<&str> = enabled_networks.iter().map(|s| s.as_str()).collect();
    let addresses =
        derive_addresses_from_mnemonic_filtered(&mnemonic, &bip39_passphrase, &network_refs)?;
    let bitcoin_account =
        initialize_bitcoin_account(&mnemonic, &bip39_passphrase, &enabled_networks)?;
    let refreshed = refresh_wallet_portfolio(
        clients.http(),
        &addresses,
        &[],
        &enabled_networks,
        bitcoin_account.as_ref(),
        fiat_currency,
        1.0,
    )
    .await;

    let wallet = Wallet {
        name: clean_name(name.unwrap_or_else(|| "Imported Wallet".to_string())),
        mnemonic,
        bip39_passphrase: std::mem::take(&mut *bip39_passphrase),
        created_at: Utc::now().to_rfc3339(),
        addresses,
        wallet_password_hash: hash_secret(&wallet_password),
        fiat_currency,
        usd_exchange_rate: refreshed.usd_exchange_rate,
        assets: refreshed.assets,
        activity: vec![activity(
            "import",
            "Wallet imported",
            "Recovery phrase verified locally",
            "Imported",
        )],
        operations: Vec::new(),
        enabled_networks,
        auto_lock_timeout_secs,
        use_crypto_symbols,
        hide_below_usd: default_hide_below_usd(),
        group_assets_by_chain: true,
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
    clients: State<'_, ProviderClients>,
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
                wallet.bip39_passphrase.clone(),
            )
        });

        if let Some((stored_hash, enabled_networks, mut mnemonic, mut bip39_passphrase)) = in_memory
        {
            if stored_hash != wallet_password_hash {
                clear_secret_string(&mut mnemonic);
                clear_secret_string(&mut bip39_passphrase);
                return Err("Invalid wallet password".to_string());
            }
            let bitcoin_account = if enabled_networks.iter().any(|network| network == "bitcoin") {
                match cached_bitcoin_account {
                    Some(account) => Ok(Some(account)),
                    None => {
                        initialize_bitcoin_account(&mnemonic, &bip39_passphrase, &enabled_networks)
                    }
                }
            } else {
                Ok(None)
            };
            clear_secret_string(&mut mnemonic);
            clear_secret_string(&mut bip39_passphrase);
            let bitcoin_account = bitcoin_account?;
            state.locked = false;
            bitcoin_account
        } else {
            let stored = read_stored_wallet(&state.storage_path)?
                .ok_or_else(|| "No wallet exists yet".to_string())?;
            let mut decrypted = decrypt_wallet(&stored, &wallet_password)?;
            if decrypted.wallet().wallet_password_hash != wallet_password_hash {
                return Err("Invalid wallet password".to_string());
            }
            refresh_filecoin_address(decrypted.wallet_mut())?;
            state.stored_wallet = Some(StoredWalletMetadata {
                wallet_name: stored.wallet_name,
            });
            let enabled_networks = decrypted.wallet().enabled_networks.clone();
            let bitcoin_account = initialize_bitcoin_account(
                &decrypted.wallet().mnemonic,
                &decrypted.wallet().bip39_passphrase,
                &enabled_networks,
            )?;
            let (wallet, key, salt) = decrypted.into_parts();
            state.encryption_key = Some(key);
            state.storage_salt = Some(salt);
            state.wallet = Some(wallet);
            state.locked = false;
            bitcoin_account
        }
    };

    let (generation, original_operations) = {
        let mut state = state.lock().map_err(|_| "State lock failed")?;
        if state.locked {
            return Err("Wallet was locked during unlock".to_string());
        }
        state.bitcoin_account = bitcoin_account;
        state.advance_wallet_generation();
        let operations = state
            .wallet
            .as_ref()
            .ok_or_else(|| "Wallet was closed during unlock".to_string())?
            .operations
            .clone();
        if let Err(error) = state.evm_nonces.restore_operations(&operations) {
            // Fail closed: a recovery failure must not leave an unlocked wallet
            // with secrets in memory while the command reports an error.
            lock_wallet_state(&mut state);
            return Err(error);
        }
        state.evm_nonces.restore_send_operations(&operations);
        (state.wallet_generation, operations)
    };
    let mut operations = original_operations.clone();
    for operation in &mut operations {
        if let Err(error) = reconcile_operation(operation, &clients).await {
            let mut state = state.lock().map_err(|_| "State lock failed")?;
            if state.wallet_generation == generation {
                lock_wallet_state(&mut state);
            }
            return Err(error);
        }
    }
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    finish_operation_recovery(&mut state, generation, &original_operations, operations)
}

fn finish_operation_recovery(
    state: &mut AppState,
    generation: u64,
    original_operations: &[Operation],
    operations: Vec<Operation>,
) -> Result<WalletRefreshResult, String> {
    let result = (|| {
        if !state.can_commit_refresh(generation) {
            return Err("Wallet session changed during operation recovery".to_string());
        }
        let wallet = state
            .wallet
            .as_mut()
            .ok_or_else(|| "Wallet was closed during recovery".to_string())?;
        if wallet.operations != original_operations {
            return Err("Operations changed during recovery".to_string());
        }
        if operations != original_operations {
            wallet.operations = operations;
            persist_state_wallet(state)?;
        }
        let operations = &state
            .wallet
            .as_ref()
            .ok_or("Wallet closed during recovery")?
            .operations;
        state.evm_nonces.restore_operations(operations)?;
        state.evm_nonces.restore_send_operations(operations);
        Ok(())
    })();
    if result.is_err() && state.wallet_generation == generation {
        lock_wallet_state(state);
    }
    result?;
    Ok(refresh_result_from_state(state, vec![]))
}

#[tauri::command]
pub(crate) fn lock_wallet(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    lock_wallet_state(&mut state);
    Ok(())
}

fn lock_wallet_state(state: &mut AppState) {
    state.evm_nonces = Default::default();
    if let Some(ref mut wallet) = state.wallet {
        clear_secret_string(&mut wallet.mnemonic);
        clear_secret_string(&mut wallet.bip39_passphrase);
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
}

#[tauri::command]
pub(crate) fn clear_wallet(state: State<'_, Mutex<AppState>>) -> Result<WalletSession, String> {
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if state.storage_path.exists() {
        fs::remove_file(&state.storage_path).map_err(|_| "Failed to remove stored wallet")?;
    }
    state.evm_nonces = Default::default();
    if let Some(ref mut wallet) = state.wallet {
        clear_secret_string(&mut wallet.mnemonic);
        clear_secret_string(&mut wallet.bip39_passphrase);
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
