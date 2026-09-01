use std::collections::HashMap;
use std::path::PathBuf;

use super::{AppState, StoredWalletMetadata, session_from_state};
use crate::dto::{Asset, FiatCurrency, Wallet};

#[test]
fn locked_session_does_not_expose_secrets() {
    let mut state = AppState::from_storage(PathBuf::from("/nonexistent/wallet.json"));
    let wallet = Wallet {
        name: "Secret Wallet".to_string(),
        mnemonic: "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        addresses: HashMap::new(),
        wallet_password_hash: "deadbeef".to_string(),
        fiat_currency: FiatCurrency::Usd,
        usd_exchange_rate: 1.0,
        assets: vec![],
        activity: vec![],
        enabled_networks: vec![],
        auto_lock_timeout_secs: None,
    };
    state.wallet = Some(wallet);
    state.locked = true;
    state.stored_wallet = Some(StoredWalletMetadata {
        wallet_name: "Secret Wallet".to_string(),
    });
    let session = session_from_state(&state);
    assert!(session.has_wallet);
    assert!(session.locked);
    assert!(session.addresses.is_none());
    assert!(session.fiat_currency.is_none());
    assert!(session.usd_exchange_rate.is_none());
    assert!(session.assets.is_empty());
    assert!(session.activity.is_empty());
}

fn unlocked_tron_state() -> AppState {
    let mut state = AppState::from_storage(PathBuf::from("/nonexistent/wallet.json"));
    state.wallet = Some(Wallet {
        name: "Tron wallet".to_string(),
        mnemonic: "test mnemonic".to_string(),
        created_at: "2026-08-28T00:00:00Z".to_string(),
        addresses: HashMap::from([(
            "tron".to_string(),
            "TUEZSdKsoDHQMeZwihtdoBiN46zxhGWYdH".to_string(),
        )]),
        wallet_password_hash: "password hash".to_string(),
        fiat_currency: FiatCurrency::Usd,
        usd_exchange_rate: 1.0,
        assets: vec![Asset {
            symbol: "TRX".to_string(),
            name: "TRON".to_string(),
            balance: "1000000".to_string(),
            decimals: 6,
            price_usd: 0.12,
            change_24h: 1.5,
            network: "tron".to_string(),
            token_address: None,
        }],
        activity: vec![],
        enabled_networks: vec!["tron".to_string()],
        auto_lock_timeout_secs: None,
    });
    state
}

#[test]
fn unlocked_session_includes_cached_tron_portfolio_data() {
    let state = unlocked_tron_state();

    let session = session_from_state(&state);

    assert_eq!(
        session
            .addresses
            .as_ref()
            .and_then(|addresses| addresses.get("tron")),
        Some(&"TUEZSdKsoDHQMeZwihtdoBiN46zxhGWYdH".to_string())
    );
    assert_eq!(session.enabled_networks, vec!["tron"]);
    assert_eq!(session.assets.len(), 1);
    assert_eq!(session.assets[0].symbol, "TRX");
    assert_eq!(session.assets[0].price_usd, 0.12);
    assert_eq!(session.fiat_currency, Some(FiatCurrency::Usd));
    assert_eq!(session.usd_exchange_rate, Some(1.0));
}

#[test]
fn stale_or_locked_wallet_cannot_commit_a_background_refresh() {
    let mut state = unlocked_tron_state();
    state.wallet_generation = 4;

    assert!(state.can_commit_refresh(4));

    state.advance_wallet_generation();
    assert!(!state.can_commit_refresh(4));

    state.locked = true;
    assert!(!state.can_commit_refresh(5));

    state.locked = false;
    state.wallet = None;
    assert!(!state.can_commit_refresh(5));
}
