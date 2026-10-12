use super::{CctpEnvironment, CctpRoute, CctpTransfer, MAX_BURN_AMOUNT};
use crate::dto::AssetIdentity;
use crate::tx::tx_status::Finality;

pub(crate) fn transfer(environment: CctpEnvironment) -> CctpTransfer {
    let route = CctpRoute::new(environment).unwrap();
    CctpTransfer::new(
        &AssetIdentity::new(&route.source.network.id, Some(route.source.usdc)),
        &AssetIdentity::new(&route.destination.network.id, Some(route.destination.usdc)),
        "0x3333333333333333333333333333333333333333",
        "0x4444444444444444444444444444444444444444",
        1_000_000,
    )
    .unwrap()
}

#[test]
fn declares_separate_mainnet_and_sepolia_deployments() {
    for (environment, chains, host) in [
        (
            CctpEnvironment::Mainnet,
            (1, 42161),
            "https://iris-api.circle.com",
        ),
        (
            CctpEnvironment::Sepolia,
            (11155111, 421614),
            "https://iris-api-sandbox.circle.com",
        ),
    ] {
        let route = CctpRoute::new(environment).unwrap();
        assert_eq!(route.source.network.chain_id().unwrap(), chains.0);
        assert_eq!(route.destination.network.chain_id().unwrap(), chains.1);
        assert_eq!((route.source.domain, route.destination.domain), (0, 3));
        assert_eq!(route.attestation_host, host);
        assert_eq!(route.required_source_finality(), Finality::Finalized);
        assert!(crate::registry::network_by_id("ethereum_sepolia").is_none());
    }
}

#[test]
fn rejects_mixed_or_invalid_deployment_configuration() {
    for index in 0..9 {
        let mut route = CctpRoute::new(CctpEnvironment::Mainnet).unwrap();
        match index {
            0 => route.source.network.chain_id = Some(11155111),
            1 => route.destination.domain = 42161,
            2 => route.source.usdc = "0x1111111111111111111111111111111111111111",
            3 => {
                route.destination.message_transmitter = "0xE737e5cEBEEBa77EFE34D4aa090756590b1CE275"
            }
            4 => route.attestation_host = "https://iris-api-sandbox.circle.com",
            5 => route.source.network.tokens[0].decimals = 18,
            6 => route.source.network.rpc_url = Some("http://example.com".into()),
            7 => route.destination.network.transaction_status_provider = None,
            _ => route.protocol_version = 1,
        }
        assert!(route.validate().is_err(), "mutation {index}");
    }
}

#[test]
fn rejects_unsupported_assets_routes_accounts_and_amounts() {
    let t = transfer(CctpEnvironment::Mainnet);
    let source = AssetIdentity::new("ethereum", Some(t.route.source.usdc));
    let destination = AssetIdentity::new("arbitrum_one", Some(t.route.destination.usdc));
    for (source, destination) in [
        (AssetIdentity::new("ethereum", None), destination.clone()),
        (
            source.clone(),
            AssetIdentity::new(
                "arbitrum_one",
                Some("0xff970a61a04b1ca14834a43f5de4533ebddb5cc8"),
            ),
        ),
        (destination.clone(), source.clone()),
        (
            source.clone(),
            AssetIdentity::new("base", Some(t.route.destination.usdc)),
        ),
        (
            source.clone(),
            AssetIdentity::new("arbitrum_sepolia", Some(t.route.destination.usdc)),
        ),
    ] {
        assert!(CctpTransfer::new(&source, &destination, t.sender(), t.recipient(), 1).is_err());
    }
    for (sender, recipient, amount) in [
        ("invalid", t.recipient(), 1),
        (t.sender(), "0x0000000000000000000000000000000000000000", 1),
        (t.sender(), t.recipient(), 0),
        (t.sender(), t.recipient(), MAX_BURN_AMOUNT + 1),
    ] {
        assert!(CctpTransfer::new(&source, &destination, sender, recipient, amount).is_err());
    }
    assert!(
        CctpTransfer::new(
            &AssetIdentity::new("ethereum", Some(&t.route.source.usdc.to_lowercase())),
            &destination,
            t.sender(),
            t.recipient(),
            MAX_BURN_AMOUNT
        )
        .is_ok()
    );
}
