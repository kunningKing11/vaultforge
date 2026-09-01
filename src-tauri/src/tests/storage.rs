use chrono::Utc;
use std::collections::HashMap;

use super::{decrypt_wallet, derive_storage_key, encrypt_wallet};
use crate::activity::{activity, hash_secret};
use crate::dto::{FiatCurrency, Wallet};
use crate::tests::starter_assets;

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
    let wallet_password = "Correct horse battery staple 42!";
    let wallet = Wallet {
        name: "Test Wallet".to_string(),
        mnemonic: "test mnemonic".to_string(),
        created_at: Utc::now().to_rfc3339(),
        addresses: HashMap::new(),
        wallet_password_hash: hash_secret(wallet_password),
        fiat_currency: FiatCurrency::Usd,
        usd_exchange_rate: 1.0,
        assets: starter_assets("ethereum"),
        activity: vec![activity("system", "Created", "Local", "1")],
        enabled_networks: vec!["evm".to_string(), "bitcoin".to_string()],
        auto_lock_timeout_secs: Some(300),
    };
    let (key, salt) = derive_storage_key(wallet_password, None).unwrap();
    let stored = encrypt_wallet(&wallet, &key, &salt).unwrap();
    assert_eq!(stored.version, 5);

    let decrypted = decrypt_wallet(&stored, wallet_password).unwrap();
    assert_eq!(decrypted.name, wallet.name);
    assert_eq!(decrypted.mnemonic, wallet.mnemonic);
    assert_eq!(decrypted.created_at, wallet.created_at);
    assert_eq!(decrypted.fiat_currency, wallet.fiat_currency);
    assert_eq!(decrypted.usd_exchange_rate, wallet.usd_exchange_rate);
    assert_eq!(decrypted.enabled_networks, wallet.enabled_networks);
    assert_eq!(
        decrypted.auto_lock_timeout_secs,
        wallet.auto_lock_timeout_secs
    );
}
