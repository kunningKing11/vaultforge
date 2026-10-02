use super::{cached_asset, token_addresses_match};
use crate::dto::AssetIdentity;
use crate::tests::starter_assets;

#[test]
fn asset_identity_keys_preserve_chain_specific_case_and_native_distinction() {
    assert_eq!(
        AssetIdentity::new("ethereum", Some("0xABC")),
        AssetIdentity::new("ethereum", Some("0xabc"))
    );
    assert_ne!(
        AssetIdentity::new("ethereum", Some("0xabc")),
        AssetIdentity::new("base", Some("0xabc"))
    );
    assert_ne!(
        AssetIdentity::new("ethereum", None),
        AssetIdentity::new("ethereum", Some("native"))
    );
    assert_ne!(
        AssetIdentity::new("solana", Some("So111")),
        AssetIdentity::new("solana", Some("so111"))
    );
    assert_ne!(
        AssetIdentity::new("tron", Some("TAbc")),
        AssetIdentity::new("tron", Some("Tabc"))
    );
}

#[test]
fn selects_cached_asset_by_network_and_token_identifier() {
    let mut assets = starter_assets("ethereum");
    let mut token = assets[0].clone();
    token.token_address = Some("0x1234".to_string());
    token.balance = "7".to_string();
    assets.insert(0, token);
    let cached = cached_asset(&assets, "ethereum", None).unwrap();
    assert_eq!(cached.symbol, "ETH");
    assert_eq!(cached.network, "ethereum");
    assert_eq!(cached.balance, "2482100000000000000000");
    assert!(cached_asset(&assets, "polygon", None).is_none());
    assert!(cached_asset(&assets, "ethereum", Some("0x5678")).is_none());
    assert_eq!(
        cached_asset(&assets, "ethereum", Some("0x1234"))
            .unwrap()
            .balance,
        "7"
    );
}

#[test]
fn token_identifiers_use_network_appropriate_case_rules() {
    assert!(token_addresses_match(
        "ethereum",
        "0xdAC17F958D2ee523a2206206994597C13D831ec7",
        "0xdac17f958d2ee523a2206206994597c13d831ec7"
    ));
    assert!(!token_addresses_match(
        "solana",
        "So11111111111111111111111111111111111111112",
        "so11111111111111111111111111111111111111112"
    ));
}
