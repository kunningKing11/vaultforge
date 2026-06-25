use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use argon2::Argon2;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

use crate::dto::{Wallet, WalletPayload};
use crate::state::{AppState, StoredWalletMetadata};

#[derive(Deserialize, Serialize)]
pub(crate) struct StoredWalletFile {
    pub(crate) version: u8,
    pub(crate) wallet_name: String,
    pub(crate) network: String,
    pub(crate) salt: String,
    pub(crate) nonce: String,
    pub(crate) ciphertext: String,
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

pub(crate) fn decrypt_wallet(
    stored: &StoredWalletFile,
    passphrase: &str,
) -> Result<Wallet, String> {
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

pub(crate) fn derive_storage_key(
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
