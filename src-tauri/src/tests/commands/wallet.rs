use std::collections::HashMap;

use super::refresh_filecoin_address;
use crate::dto::{FiatCurrency, Wallet};

#[test]
fn unlock_migrates_legacy_filecoin_addresses() {
    let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let mut wallet = Wallet {
        name: "Test Wallet".to_string(),
        mnemonic: mnemonic.to_string(),
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
        enabled_networks: vec!["filecoin".to_string()],
        auto_lock_timeout_secs: None,
    };
    refresh_filecoin_address(&mut wallet).unwrap();
    assert_eq!(
        wallet.addresses.get("filecoin").unwrap(),
        "f1qode47ievxlxzk6z2viuovedabmn3tq6t57uqhq"
    );
}
