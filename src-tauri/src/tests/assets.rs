use super::{cached_asset, token_addresses_match};
use crate::tests::starter_assets;

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

#[test]
fn selects_cached_asset_by_network_and_symbol() {
    let assets = starter_assets("ethereum");
    let cached = cached_asset(&assets, "ethereum", "ETH").unwrap();
    assert_eq!(cached.symbol, "ETH");
    assert_eq!(cached.network, "ethereum");
    assert_eq!(cached.balance, "2482100000000000000000");
    assert!(cached_asset(&assets, "polygon", "ETH").is_none());
    assert!(cached_asset(&assets, "ethereum", "MATIC").is_none());
}
