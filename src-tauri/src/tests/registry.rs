use std::collections::HashSet;

use super::{evm_networks, network_by_id, registry};

#[test]
fn registry_has_unique_network_and_token_identifiers() {
    let mut network_ids = HashSet::new();
    for network in &registry().networks {
        assert!(network_ids.insert(&network.id));
        let mut symbols = HashSet::new();
        let mut contracts = HashSet::new();
        for token in &network.tokens {
            assert!(symbols.insert(&token.symbol));
            assert!(
                contracts.insert(
                    token
                        .token_address
                        .as_deref()
                        .expect("configured token must have an address")
                        .to_ascii_lowercase()
                )
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
            _ => assert!(
                rpc_url.contains("publicnode.com"),
                "{} should use PublicNode",
                network.id
            ),
        }
    }
    assert_eq!(evm_networks().count(), 8);
}

#[test]
fn polygon_uses_pol_and_distinguishes_native_and_bridged_usdc() {
    let polygon = network_by_id("polygon").unwrap();
    assert_eq!(polygon.native_asset.symbol, "POL");
    assert!(polygon.tokens.iter().any(|token| token.symbol == "USDC"));
    assert!(polygon.tokens.iter().any(|token| token.symbol == "USDC.e"));
}
