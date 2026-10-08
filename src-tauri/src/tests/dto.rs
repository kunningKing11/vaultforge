use super::{Activity, AssetIdentity, FiatCurrency, WalletPayload, default_enabled_networks};
use crate::activity::activity;
use crate::derivation::derive_addresses_from_mnemonic_filtered;

#[test]
fn activity_identity_is_optional_for_old_wallets_and_round_trips_for_new_entries() {
    let mut entry = activity("send", "Sent", "", "1 ETH");
    let legacy_json = serde_json::to_value(&entry).unwrap();
    assert!(legacy_json.get("asset").is_none());
    assert!(legacy_json.get("counter_asset").is_none());
    let legacy: Activity = serde_json::from_value(legacy_json).unwrap();
    assert!(legacy.asset.is_none());
    assert!(legacy.counter_asset.is_none());

    entry.asset = Some(AssetIdentity::new("ethereum", Some("0xABC")));
    entry.counter_asset = Some(AssetIdentity::new("base", Some("0xABC")));
    let restored: Activity = serde_json::from_str(&serde_json::to_string(&entry).unwrap()).unwrap();
    assert_eq!(restored.asset, entry.asset);
    assert_eq!(restored.counter_asset, entry.counter_asset);
}

#[test]
fn decrypts_legacy_wallet_password_hash_payloads() {
    let payload: WalletPayload = serde_json::from_str(
        r#"{
            "wallet_name":"Test Wallet",
            "mnemonic":"abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
            "created_at":"2026-01-01T00:00:00Z",
            "address":"0x0000000000000000000000000000000000000000",
            "addresses":{},
            "passphrase_hash":"legacy-hash",
            "assets":[],
            "activity":[]
        }"#,
    )
    .unwrap();

    assert_eq!(payload.wallet_password_hash, "legacy-hash");
    assert_eq!(payload.fiat_currency, FiatCurrency::Usd);
    assert_eq!(payload.usd_exchange_rate, 1.0);
    assert!(!payload.use_crypto_symbols);
    assert!(payload.bip39_passphrase.is_empty());
    assert!(payload.operations.is_empty());
}

#[test]
fn default_enabled_networks_derives_evm_and_tron_addresses() {
    let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let enabled_networks = default_enabled_networks();
    let enabled: Vec<&str> = enabled_networks.iter().map(String::as_str).collect();
    let addresses = derive_addresses_from_mnemonic_filtered(mnemonic, "", &enabled).unwrap();

    assert!(addresses.contains_key("evm"));
    assert!(addresses.contains_key("tron"));
}

#[test]
fn deserializes_frontend_fiat_currency_codes() {
    assert_eq!(
        serde_json::from_str::<FiatCurrency>(r#""EUR""#).unwrap(),
        FiatCurrency::Eur
    );
}
