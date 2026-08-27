use crate::dto::{RefreshWarning, Wallet, WalletRefreshResult, WalletSession};
use crate::providers::bitcoin::BitcoinAccountSnapshot;
use crate::storage::read_stored_wallet;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

pub(crate) fn clear_secret_string(s: &mut str) {
    let buf = unsafe { s.as_bytes_mut() };
    buf.fill(0);
}

pub(crate) fn validate_unlocked(state: &State<'_, Mutex<AppState>>) -> Result<(), String> {
    let state = state.lock().map_err(|_| "State lock failed")?;
    if state.wallet.is_none() {
        return Err("No wallet exists yet".to_string());
    }
    if state.locked {
        return Err("Wallet is locked".to_string());
    }
    Ok(())
}

pub(crate) struct AppState {
    pub(crate) wallet: Option<Wallet>,
    pub(crate) locked: bool,
    pub(crate) stored_wallet: Option<StoredWalletMetadata>,
    pub(crate) encryption_key: Option<[u8; 32]>,
    pub(crate) storage_salt: Option<Vec<u8>>,
    pub(crate) bitcoin_account: Option<BitcoinAccountSnapshot>,
    pub(crate) storage_path: PathBuf,
}

#[derive(Clone)]
pub(crate) struct StoredWalletMetadata {
    pub(crate) wallet_name: String,
}

impl AppState {
    pub(crate) fn from_storage(storage_path: PathBuf) -> Self {
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
            bitcoin_account: None,
            storage_path,
        }
    }
}

pub(crate) fn session_from_state(state: &AppState) -> WalletSession {
    let Some(wallet) = state.wallet.as_ref() else {
        if let Some(stored_wallet) = state.stored_wallet.as_ref() {
            return WalletSession {
                has_wallet: true,
                locked: true,
                wallet_name: Some(stored_wallet.wallet_name.clone()),
                addresses: None,
                assets: vec![],
                activity: vec![],
                enabled_networks: vec![],
                auto_lock_timeout_secs: None,
            };
        }

        return WalletSession {
            has_wallet: false,
            locked: false,
            wallet_name: None,
            addresses: None,
            assets: vec![],
            activity: vec![],
            enabled_networks: vec![],
            auto_lock_timeout_secs: None,
        };
    };

    if state.locked {
        return WalletSession {
            has_wallet: true,
            locked: true,
            wallet_name: Some(wallet.name.clone()),
            addresses: None,
            assets: vec![],
            activity: vec![],
            enabled_networks: vec![],
            auto_lock_timeout_secs: None,
        };
    }

    let mut addresses = wallet.addresses.clone();
    if let Some(bitcoin_address) = state
        .bitcoin_account
        .as_ref()
        .and_then(|account| account.next_receive_address())
    {
        addresses.insert("bitcoin".to_string(), bitcoin_address.address.clone());
    }

    WalletSession {
        has_wallet: true,
        locked: false,
        wallet_name: Some(wallet.name.clone()),
        addresses: Some(addresses),
        assets: wallet.assets.clone(),
        activity: wallet.activity.clone(),
        enabled_networks: wallet.enabled_networks.clone(),
        auto_lock_timeout_secs: wallet.auto_lock_timeout_secs,
    }
}

pub(crate) fn refresh_result_from_state(
    state: &AppState,
    warnings: Vec<RefreshWarning>,
) -> WalletRefreshResult {
    WalletRefreshResult {
        session: session_from_state(state),
        warnings,
    }
}
