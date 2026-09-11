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
        use_crypto_symbols: true,
    };
    let (key, salt) = derive_storage_key(wallet_password, None).unwrap();
    let mut stored = encrypt_wallet(&wallet, &key, &salt).unwrap();
    assert_eq!(stored.version, 6);

    let decrypted = decrypt_wallet(&stored, wallet_password).unwrap();
    assert_eq!(decrypted.wallet().name, wallet.name);
    assert_eq!(decrypted.wallet().mnemonic, wallet.mnemonic);
    assert_eq!(decrypted.wallet().created_at, wallet.created_at);
    assert_eq!(decrypted.wallet().fiat_currency, wallet.fiat_currency);
    assert_eq!(
        decrypted.wallet().usd_exchange_rate,
        wallet.usd_exchange_rate
    );
    assert_eq!(decrypted.wallet().enabled_networks, wallet.enabled_networks);
    assert_eq!(
        decrypted.wallet().auto_lock_timeout_secs,
        wallet.auto_lock_timeout_secs
    );
    assert_eq!(
        decrypted.wallet().use_crypto_symbols,
        wallet.use_crypto_symbols
    );
    let (decrypted_wallet, decrypted_key, decrypted_salt) = decrypted.into_parts();
    assert_eq!(decrypted_wallet.mnemonic, wallet.mnemonic);
    assert_eq!(decrypted_key, key);
    assert_eq!(decrypted_salt, salt);

    stored.version = 5;
    let migrated = decrypt_wallet(&stored, wallet_password).unwrap();
    assert!(!migrated.wallet().use_crypto_symbols);
}
