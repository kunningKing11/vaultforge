use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use argon2::Argon2;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use zeroize::Zeroize;

use crate::dto::{FiatCurrency, Wallet, WalletPayload};
use crate::state::{AppState, StoredWalletMetadata, clear_secret_string};

#[derive(Deserialize, Serialize)]
pub(crate) struct StoredWalletFile {
    pub(crate) version: u8,
    pub(crate) wallet_name: String,
    pub(crate) network: String,
    pub(crate) salt: String,
    pub(crate) nonce: String,
    pub(crate) ciphertext: String,
}

pub(crate) struct DecryptedWallet {
    wallet: Option<Wallet>,
    key: Option<[u8; 32]>,
    salt: Option<Vec<u8>>,
}

impl DecryptedWallet {
    pub(crate) fn wallet(&self) -> &Wallet {
        self.wallet.as_ref().expect("decrypted wallet is present")
    }

    pub(crate) fn wallet_mut(&mut self) -> &mut Wallet {
        self.wallet.as_mut().expect("decrypted wallet is present")
    }

    pub(crate) fn into_parts(mut self) -> (Wallet, [u8; 32], Vec<u8>) {
        (
            self.wallet.take().expect("decrypted wallet is present"),
            self.key.take().expect("decryption key is present"),
            self.salt.take().expect("storage salt is present"),
        )
    }

    fn zeroize_remaining(&mut self) {
        if let Some(wallet) = &mut self.wallet {
            clear_secret_string(&mut wallet.mnemonic);
        }
        if let Some(key) = &mut self.key {
            key.zeroize();
        }
        if let Some(salt) = &mut self.salt {
            salt.zeroize();
        }
    }
}

impl Drop for DecryptedWallet {
    fn drop(&mut self) {
        self.zeroize_remaining();
    }
}

pub(crate) fn read_stored_wallet(path: &PathBuf) -> Result<Option<StoredWalletFile>, String> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path).map_err(|_| "Failed to read stored wallet")?;
    let stored = serde_json::from_str(&contents).map_err(|_| "Stored wallet file is invalid")?;
    Ok(Some(stored))
}

pub(crate) fn persist_state_wallet(state: &mut AppState) -> Result<(), String> {
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

pub(crate) fn encrypt_wallet(
    wallet: &Wallet,
    key: &[u8; 32],
    salt: &[u8],
) -> Result<StoredWalletFile, String> {
    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill(&mut nonce_bytes);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "Failed to initialize encryption")?;
    let payload = WalletPayload {
        wallet_name: wallet.name.clone(),
        mnemonic: wallet.mnemonic.clone(),
        created_at: wallet.created_at.clone(),
        addresses: wallet.addresses.clone(),
        wallet_password_hash: wallet.wallet_password_hash.clone(),
        fiat_currency: wallet.fiat_currency,
        usd_exchange_rate: wallet.usd_exchange_rate,
        assets: wallet.assets.clone(),
        activity: wallet.activity.clone(),
        enabled_networks: wallet.enabled_networks.clone(),
        auto_lock_timeout_secs: wallet.auto_lock_timeout_secs,
        use_crypto_symbols: wallet.use_crypto_symbols,
    };
    let plaintext = serde_json::to_vec(&payload).map_err(|_| "Failed to encode wallet")?;
    let nonce = Nonce::try_from(nonce_bytes.as_slice()).map_err(|_| "Failed to create nonce")?;
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_ref())
        .map_err(|_| "Failed to encrypt wallet")?;

    Ok(StoredWalletFile {
        version: 6,
        wallet_name: wallet.name.clone(),
        network: "ethereum".to_string(),
        salt: BASE64.encode(salt),
        nonce: BASE64.encode(nonce_bytes),
        ciphertext: BASE64.encode(ciphertext),
    })
}

pub(crate) fn decrypt_wallet(
    stored: &StoredWalletFile,
    wallet_password: &str,
) -> Result<DecryptedWallet, String> {
    if !matches!(stored.version, 2..=6) {
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
    let (mut key, salt) = derive_storage_key(wallet_password, Some(&salt))?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| {
        key.zeroize();
        "Failed to initialize encryption".to_string()
    })?;
    let nonce = Nonce::try_from(nonce.as_slice()).map_err(|_| {
        key.zeroize();
        "Stored wallet nonce is invalid".to_string()
    })?;
    let mut plaintext = cipher.decrypt(&nonce, ciphertext.as_ref()).map_err(|_| {
        key.zeroize();
        "Invalid wallet password".to_string()
    })?;
    let payload: WalletPayload = serde_json::from_slice(&plaintext).map_err(|_| {
        key.zeroize();
        plaintext.zeroize();
        "Stored wallet contents are invalid".to_string()
    })?;
    plaintext.zeroize();
    Ok(DecryptedWallet {
        wallet: Some(Wallet {
            name: payload.wallet_name,
            mnemonic: payload.mnemonic,
            created_at: payload.created_at,
            addresses: payload.addresses,
            wallet_password_hash: payload.wallet_password_hash,
            fiat_currency: if stored.version >= 5 {
                payload.fiat_currency
            } else {
                FiatCurrency::Usd
            },
            usd_exchange_rate: if stored.version >= 5 {
                payload.usd_exchange_rate
            } else {
                1.0
            },
            assets: payload.assets,
            activity: payload.activity,
            enabled_networks: payload.enabled_networks,
            auto_lock_timeout_secs: payload.auto_lock_timeout_secs,
            use_crypto_symbols: if stored.version >= 6 {
                payload.use_crypto_symbols
            } else {
                false
            },
        }),
        key: Some(key),
        salt: Some(salt),
    })
}

pub(crate) fn derive_storage_key(
    wallet_password: &str,
    salt: Option<&[u8]>,
) -> Result<([u8; 32], Vec<u8>), String> {
    let salt = salt.map(|value| value.to_vec()).unwrap_or_else(|| {
        let mut salt = vec![0u8; 16];
        rand::rng().fill(salt.as_mut_slice());
        salt
    });
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(wallet_password.as_bytes(), &salt, &mut key)
        .map_err(|_| "Failed to derive wallet encryption key")?;
    Ok((key, salt))
}
#[cfg(test)]
#[path = "tests/storage.rs"]
mod tests;
