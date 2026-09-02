use super::{FiatCurrency, WalletPayload, default_enabled_networks};
use crate::derivation::derive_addresses_from_mnemonic_filtered;

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
}

#[test]
fn deserializes_frontend_fiat_currency_codes() {
    assert_eq!(
        serde_json::from_str::<FiatCurrency>(r#""EUR""#).unwrap(),
        FiatCurrency::Eur
    );
}

#[test]
fn default_enabled_networks_derives_evm_and_tron_addresses() {
    let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let enabled_networks = default_enabled_networks();
    let enabled: Vec<&str> = enabled_networks.iter().map(String::as_str).collect();
    let addresses = derive_addresses_from_mnemonic_filtered(mnemonic, &enabled).unwrap();

    assert!(addresses.contains_key("evm"));
    assert!(addresses.contains_key("tron"));
}
