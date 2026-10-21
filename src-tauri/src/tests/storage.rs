use chrono::Utc;
use std::collections::HashMap;

use super::{
    decrypt_wallet, derive_storage_key, encrypt_wallet, persist_state_wallet, read_stored_wallet,
};
use crate::activity::{activity, hash_secret};
use crate::dto::{AssetIdentity, FiatCurrency, Wallet, WalletPayload};
use crate::operations::{CctpData, OperationStep, TransactionStatus, TransactionStep};
use crate::state::AppState;
use crate::tests::{sample_attempt, sample_operation, starter_assets};
use crate::tx::tx_status::{Execution, Finality, TxStatus};

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
    let mut wallet = Wallet {
        name: "Test Wallet".to_string(),
        mnemonic: "test mnemonic".to_string(),
        bip39_passphrase: "TREZOR".to_string(),
        created_at: Utc::now().to_rfc3339(),
        addresses: HashMap::new(),
        wallet_password_hash: hash_secret(wallet_password),
        fiat_currency: FiatCurrency::Usd,
        usd_exchange_rate: 1.0,
        assets: starter_assets("ethereum"),
        activity: vec![activity("system", "Created", "Local", "1")],
        operations: vec![sample_operation()],
        enabled_networks: vec!["evm".to_string(), "bitcoin".to_string()],
        auto_lock_timeout_secs: Some(300),
        use_crypto_symbols: true,
        hide_below_usd: crate::dto::default_hide_below_usd(),
    };
    wallet.activity[0].asset = Some(AssetIdentity::new("ethereum", Some("0xABC")));
    wallet.activity[0].counter_asset = Some(AssetIdentity::new("base", Some("0xABC")));
    wallet.operations[0].step = OperationStep::AwaitingAttestation;
    wallet.operations[0].attempts.push(sample_attempt(
        TransactionStep::Burn,
        TransactionStatus::Confirmed,
    ));
    wallet.operations[0].cctp = Some(CctpData {
        message: vec![1, 2],
        attestation: Some(vec![3, 4]),
        ..CctpData::default()
    });
    let mut observation = TxStatus::new(Execution::Succeeded, Finality::Finalized);
    observation.block_height = Some(42);
    observation.block_hash = Some(format!("0x{}", "ab".repeat(32)));
    observation.confirmations = Some(20);
    wallet.activity[0].tx_status = Some(observation.clone());
    wallet.operations[0].attempts[0].tx_status = Some(observation);
    let (key, salt) = derive_storage_key(wallet_password, None).unwrap();
    let mut stored = encrypt_wallet(&wallet, &key, &salt).unwrap();
    assert_eq!(stored.version, 15);
    assert!(!serde_json::to_string(&stored).unwrap().contains("TREZOR"));

    let decrypted = decrypt_wallet(&stored, wallet_password).unwrap();
    assert_eq!(decrypted.wallet().name, wallet.name);
    assert_eq!(decrypted.wallet().operations, wallet.operations);
    assert_eq!(
        decrypted.wallet().activity[0].tx_status,
        wallet.activity[0].tx_status
    );
    assert_eq!(decrypted.wallet().mnemonic, wallet.mnemonic);
    assert_eq!(decrypted.wallet().bip39_passphrase, wallet.bip39_passphrase);
    assert_eq!(decrypted.wallet().created_at, wallet.created_at);
    assert_eq!(
        decrypted.wallet().activity[0].asset,
        wallet.activity[0].asset
    );
    assert_eq!(
        decrypted.wallet().activity[0].counter_asset,
        wallet.activity[0].counter_asset
    );
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

    let mut ordinary_wallet = wallet.clone();
    ordinary_wallet.bip39_passphrase.clear();
    let ordinary_stored = encrypt_wallet(&ordinary_wallet, &key, &salt).unwrap();
    assert_eq!(ordinary_stored.version, 15);
    assert!(
        decrypt_wallet(&ordinary_stored, wallet_password)
            .unwrap()
            .wallet()
            .bip39_passphrase
            .is_empty()
    );

    stored.version = 14;
    assert!(decrypt_wallet(&stored, wallet_password).is_ok());

    stored.version = 12;
    let legacy = decrypt_wallet(&stored, wallet_password).unwrap();
    assert!(
        legacy
            .wallet()
            .operations
            .iter()
            .all(|operation| operation.swap.is_none())
    );

    stored.version = 5;
    let migrated = decrypt_wallet(&stored, wallet_password).unwrap();
    assert!(!migrated.wallet().use_crypto_symbols);

    for version in [1, 16] {
        stored.version = version;
        assert_eq!(
            decrypt_wallet(&stored, wallet_password).err().unwrap(),
            "Unsupported wallet version"
        );
    }

    let directory = std::env::temp_dir().join(format!(
        "vaultforge-operation-test-{}",
        crate::activity::random_hex(16)
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("wallet.json");
    let mut state = AppState::from_storage(path.clone());
    state.wallet = Some(wallet.clone());
    state.encryption_key = Some(key);
    state.storage_salt = Some(salt.clone());
    persist_state_wallet(&mut state).unwrap();
    drop(state);
    let restarted = AppState::from_storage(path.clone());
    assert!(restarted.locked);
    let saved = read_stored_wallet(&path).unwrap().unwrap();
    let restored = decrypt_wallet(&saved, wallet_password).unwrap();
    assert_eq!(restored.wallet().operations, wallet.operations);
    assert_eq!(restored.wallet().bip39_passphrase, wallet.bip39_passphrase);
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&directory).unwrap();
}

#[test]
fn legacy_payloads_default_the_hide_below_cutoff_to_one_dollar() {
    let payload: WalletPayload = serde_json::from_value(serde_json::json!({
        "wallet_name": "Legacy",
        "mnemonic": "test mnemonic",
        "created_at": "2025-01-01T00:00:00Z",
        "addresses": {},
        "wallet_password_hash": "hash",
        "assets": [],
        "activity": [],
    }))
    .unwrap();
    assert_eq!(payload.hide_below_usd, 1.0);
}
