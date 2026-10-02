use std::collections::HashSet;

use super::{NetworkRegistry, configured_asset, evm_networks, network_by_id, registry};
use crate::dto::AssetIdentity;

#[test]
fn configured_assets_match_case_only_on_evm() {
    let ethereum = network_by_id("ethereum").unwrap();
    let address = ethereum.tokens[0].token_address.as_deref().unwrap();
    assert!(configured_asset(ethereum, Some(&address.to_ascii_lowercase())).is_some());
    let mut source: NetworkRegistry =
        serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/networks.json"))).unwrap();
    let solana = source
        .networks
        .iter_mut()
        .find(|network| network.id == "solana")
        .unwrap();
    let mut token = source_token();
    token.token_address = Some("So11111111111111111111111111111111111111112".into());
    solana.tokens = vec![token];
    assert!(
        configured_asset(&solana, Some("So11111111111111111111111111111111111111112")).is_some()
    );
    assert!(
        configured_asset(&solana, Some("so11111111111111111111111111111111111111112")).is_none()
    );
}

fn source_token() -> super::AssetConfig {
    serde_json::from_value(serde_json::json!({
        "symbol": "SOL", "name": "Example token", "decimals": 9, "coinGeckoId": "example"
    }))
    .unwrap()
}

#[test]
fn polygon_uses_pol_and_distinguishes_native_and_bridged_usdc() {
    let polygon = network_by_id("polygon").unwrap();
    assert_eq!(polygon.native_asset.symbol, "POL");
    assert!(polygon.tokens.iter().any(|token| token.symbol == "USDC"));
    assert!(polygon.tokens.iter().any(|token| token.symbol == "USDC.e"));
}

#[test]
fn registry_exposes_configured_unicode_symbols() {
    let bitcoin = network_by_id("bitcoin").unwrap();
    let ethereum = network_by_id("ethereum").unwrap();
    let zcash = network_by_id("zcash").unwrap();

    assert_eq!(bitcoin.native_asset.unicode_symbol.as_deref(), Some("₿"));
    assert_eq!(ethereum.native_asset.unicode_symbol.as_deref(), Some("Ξ"));
    assert_eq!(zcash.native_asset.unicode_symbol.as_deref(), Some("ⓩ"));
}

#[test]
fn registry_has_unique_network_and_token_identifiers() {
    let mut network_ids = HashSet::new();
    for network in &registry().networks {
        assert!(network_ids.insert(&network.id));
        let mut contracts = HashSet::new();
        for token in &network.tokens {
            assert!(
                contracts.insert(AssetIdentity::new(
                    &network.id,
                    Some(
                        token
                            .token_address
                            .as_deref()
                            .expect("configured token must have an address")
                    )
                ))
            );
        }
    }
}

#[test]
fn supported_rpc_providers_use_expected_hosts() {
    for network in &registry().networks {
        let Some(rpc_url) = network.rpc_url.as_deref() else {
            continue;
        };
        match network.id.as_str() {
            "monad" => assert_eq!(rpc_url, "https://rpc.monad.xyz"),
            "solana" => assert_eq!(rpc_url, "https://api.mainnet.solana.com"),
            "xrpl" => assert_eq!(rpc_url, "https://xrplcluster.com/"),
            _ => assert!(
                rpc_url.contains("publicnode.com"),
                "{} should use PublicNode",
                network.id
            ),
        }
    }
    assert_eq!(evm_networks().count(), 8);
}
