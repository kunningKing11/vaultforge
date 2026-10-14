use std::collections::HashMap;
use std::path::PathBuf;

use super::{
    apply_wallet_settings, finish_operation_recovery, refresh_filecoin_address,
    validate_hide_below_usd,
};
use crate::dto::{FiatCurrency, Wallet};
use crate::state::{AppState, StoredWalletMetadata};
use crate::tests::sample_operation;

fn recovery_state(path: PathBuf) -> AppState {
    let mut state = AppState::from_storage(path);
    state.wallet = Some(Wallet {
        name: "Recovery wallet".to_string(),
        mnemonic: "test secret mnemonic".to_string(),
        bip39_passphrase: "test secret passphrase".to_string(),
        created_at: "2026-10-08T00:00:00Z".to_string(),
        addresses: HashMap::new(),
        wallet_password_hash: "test hash".to_string(),
        fiat_currency: FiatCurrency::Usd,
        usd_exchange_rate: 1.0,
        assets: vec![],
        activity: vec![],
        operations: vec![sample_operation()],
        enabled_networks: vec!["ethereum".to_string(), "base".to_string()],
        auto_lock_timeout_secs: Some(60),
        use_crypto_symbols: false,
        hide_below_usd: crate::dto::default_hide_below_usd(),
    });
    state.stored_wallet = Some(StoredWalletMetadata {
        wallet_name: "Recovery wallet".to_string(),
    });
    state.encryption_key = Some([42; 32]);
    state.storage_salt = Some(vec![1; 16]);
    state.wallet_generation = 7;
    state.locked = false;
    state
}

#[test]
fn applies_all_wallet_settings_together() {
    let mut wallet = Wallet {
        name: "Old name".to_string(),
        mnemonic: String::new(),
        bip39_passphrase: String::new(),
        created_at: String::new(),
        addresses: HashMap::new(),
        wallet_password_hash: String::new(),
        fiat_currency: FiatCurrency::Usd,
        usd_exchange_rate: 1.0,
        assets: vec![],
        activity: vec![],
        operations: Vec::new(),
        enabled_networks: vec![],
        auto_lock_timeout_secs: None,
        use_crypto_symbols: false,
        hide_below_usd: crate::dto::default_hide_below_usd(),
    };

    apply_wallet_settings(
        &mut wallet,
        "  Renamed wallet  ".to_string(),
        FiatCurrency::Eur,
        0.92,
        Some(300),
        true,
        2.5,
    );

    assert_eq!(wallet.name, "Renamed wallet");
    assert_eq!(wallet.fiat_currency, FiatCurrency::Eur);
    assert_eq!(wallet.usd_exchange_rate, 0.92);
    assert_eq!(wallet.auto_lock_timeout_secs, Some(300));
    assert!(wallet.use_crypto_symbols);
    assert_eq!(wallet.hide_below_usd, 2.5);
}

#[test]
fn recovery_for_an_old_generation_does_not_lock_new_wallet() {
    let mut state = recovery_state(PathBuf::from("/nonexistent/wallet.json"));
    let original = state.wallet.as_ref().unwrap().operations.clone();
    state.advance_wallet_generation();
    assert!(finish_operation_recovery(&mut state, 7, &original, original.clone()).is_err());
    assert!(!state.locked);
    assert!(state.wallet.is_some());
    assert!(state.encryption_key.is_some());
    assert_eq!(state.wallet_generation, 8);
}

#[test]
fn recovery_persistence_failure_relocks_and_clears_secrets() {
    let path = std::env::temp_dir().join(format!(
        "vaultforge-recovery-failure-{}",
        crate::activity::random_hex(16)
    ));
    std::fs::create_dir(&path).unwrap();
    let mut state = recovery_state(path.clone());
    state.evm_nonces.reserve(1, "0xabc", 7).unwrap();
    let original = state.wallet.as_ref().unwrap().operations.clone();
    let mut recovered = original.clone();
    recovered[0].last_error = Some("provider error".to_string());
    let error = finish_operation_recovery(&mut state, 7, &original, recovered)
        .err()
        .unwrap();
    assert_eq!(error, "Failed to save wallet");
    assert!(state.locked);
    assert!(state.wallet.is_none());
    assert!(state.encryption_key.is_none());
    assert!(state.storage_salt.is_none());
    assert!(state.bitcoin_account.is_none());
    assert_eq!(state.wallet_generation, 8);
    assert!(state.evm_nonces.reserve(1, "0xabc", 7).is_ok());
    assert!(state.stored_wallet.is_some());
    std::fs::remove_dir(path).unwrap();
}

#[test]
fn recovery_snapshot_conflict_relocks_the_same_wallet() {
    let mut state = recovery_state(PathBuf::from("/nonexistent/wallet.json"));
    let original = state.wallet.as_ref().unwrap().operations.clone();
    state.wallet.as_mut().unwrap().operations[0].last_error = Some("changed".to_string());
    assert!(finish_operation_recovery(&mut state, 7, &original, original.clone()).is_err());
    assert!(state.locked);
    assert!(state.wallet.is_none());
    assert!(state.encryption_key.is_none());
}

#[test]
fn recovery_without_changes_returns_unlocked_session_without_writing() {
    let path = std::env::temp_dir().join(format!(
        "vaultforge-recovery-no-write-{}",
        crate::activity::random_hex(16)
    ));
    let mut state = recovery_state(path.clone());
    let original = state.wallet.as_ref().unwrap().operations.clone();
    let result = finish_operation_recovery(&mut state, 7, &original, original.clone()).unwrap();
    assert!(!result.session.locked);
    assert_eq!(result.session.auto_lock_timeout_secs, Some(60));
    assert!(!path.exists());
    assert!(state.wallet.is_some());
}

#[test]
fn rejects_invalid_hide_below_usd_settings() {
    assert!(validate_hide_below_usd(0.0).is_ok());
    assert!(validate_hide_below_usd(2.5).is_ok());
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01] {
        assert!(validate_hide_below_usd(invalid).is_err());
    }
}

#[test]
fn unlock_migrates_legacy_filecoin_addresses() {
    let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let mut wallet = Wallet {
        name: "Test Wallet".to_string(),
        mnemonic: mnemonic.to_string(),
        bip39_passphrase: String::new(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        addresses: HashMap::from([(
            "filecoin".to_string(),
            "f1fFXqnEMPFe1NoAajxRKukEBLwshG1LQQC".to_string(),
        )]),
        wallet_password_hash: "unused".to_string(),
        fiat_currency: FiatCurrency::Usd,
        usd_exchange_rate: 1.0,
        assets: vec![],
        activity: vec![],
        operations: Vec::new(),
        enabled_networks: vec!["filecoin".to_string()],
        auto_lock_timeout_secs: None,
        use_crypto_symbols: false,
        hide_below_usd: crate::dto::default_hide_below_usd(),
    };
    refresh_filecoin_address(&mut wallet).unwrap();
    assert_eq!(
        wallet.addresses.get("filecoin").unwrap(),
        "f1qode47ievxlxzk6z2viuovedabmn3tq6t57uqhq"
    );
}
