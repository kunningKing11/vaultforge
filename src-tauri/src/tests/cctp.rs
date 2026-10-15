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
fn binds_all_solana_directions_with_native_recipient_and_caller_policies() {
    let solana = &super::SOLANA_DEPLOYMENT;
    let mut directions = 0;
    for evm in super::MAINNET_DEPLOYMENTS {
        for (source, destination, sender, recipient) in [
            (
                solana,
                evm,
                "Vote111111111111111111111111111111111111111",
                "0x4444444444444444444444444444444444444444",
            ),
            (
                evm,
                solana,
                "0x3333333333333333333333333333333333333333",
                "Vote111111111111111111111111111111111111111",
            ),
        ] {
            let mut operation = crate::operations::create_operation(
                AssetIdentity::new(source.id, Some(source.usdc)),
                AssetIdentity::new(destination.id, Some(destination.usdc)),
                sender.into(),
                recipient.into(),
                1000000,
            )
            .unwrap();
            let transfer = operation.cctp_transfer().unwrap();
            assert_eq!(transfer.caller_word().unwrap(), [0; 32]);
            let mut burn = crate::tests::sample_attempt(
                crate::operations::TransactionStep::Burn,
                crate::operations::TransactionStatus::Submitted,
            );
            if source.id == "solana" {
                burn.tx_hash = bs58::encode([1; 64]).into_string();
            }
            operation.attempts.push(burn.clone());
            let mut evidence = crate::cctp_attestation::tests::evidence(&operation);
            if source.id == "solana" {
                evidence.transaction_hash = burn.tx_hash;
                evidence.block_height = None;
                evidence.block_hash = None;
                evidence.message_log_index = None;
                evidence.deposit_log_index = None;
                evidence.slot = Some(42);
                evidence.message_account = Some(sender.into());
            }
            let message = crate::cctp_attestation::tests::attested(&operation);
            crate::cctp_attestation::validate_bound_message(&operation, &evidence, &message)
                .unwrap();
            if destination.id == "solana" {
                assert_ne!(
                    transfer.mint_recipient_word().unwrap(),
                    crate::cctp::address_word(&transfer.route.destination.network, recipient)
                        .unwrap()
                );
            }
            assert_eq!(
                transfer.route.source.network.chain_id,
                if source.id == "solana" {
                    None
                } else {
                    source.evm_chain_id
                }
            );
            let mut wrong = operation.source_asset.clone();
            if source.id == "solana" {
                wrong.token_address = Some(source.usdc.to_ascii_lowercase());
                assert!(
                    CctpTransfer::new(
                        &wrong,
                        &operation.destination_asset,
                        sender,
                        recipient,
                        1000000
                    )
                    .is_err()
                );
            }
            directions += 1;
        }
    }
    assert_eq!(directions, 14);
}

#[test]
fn builds_and_binds_all_supported_mainnet_evm_directions() {
    let deployments = super::MAINNET_DEPLOYMENTS;
    let mut routes = 0;
    for source in deployments {
        for destination in deployments {
            if source.id == destination.id {
                assert!(CctpRoute::for_networks(source.id, destination.id).is_err());
                continue;
            }
            let transfer = CctpTransfer::new(
                &AssetIdentity::new(source.id, Some(source.usdc)),
                &AssetIdentity::new(destination.id, Some(destination.usdc)),
                "0x3333333333333333333333333333333333333333",
                "0x4444444444444444444444444444444444444444",
                1000000,
            )
            .unwrap();
            assert_eq!(transfer.route.source.domain, source.domain);
            assert_eq!(transfer.route.destination.domain, destination.domain);
            let call = crate::tx::cctp::build_burn_call(&transfer).unwrap();
            assert_eq!(
                &call.data[36..68],
                &crate::tx::evm::evm_uint_word(destination.domain as u128)
            );
            assert_eq!(
                transfer.route.source.network.chain_id().unwrap(),
                source.evm_chain_id.unwrap()
            );
            assert_eq!(
                transfer.route.source.network.native_asset.symbol,
                source.symbol
            );
            let mut operation = crate::operations::create_operation(
                AssetIdentity::new(source.id, Some(source.usdc)),
                AssetIdentity::new(destination.id, Some(destination.usdc)),
                transfer.sender().into(),
                transfer.recipient().into(),
                1000000,
            )
            .unwrap();
            operation.attempts.push(crate::tests::sample_attempt(
                crate::operations::TransactionStep::Burn,
                crate::operations::TransactionStatus::Submitted,
            ));
            let evidence = crate::cctp_attestation::tests::evidence(&operation);
            let message = crate::cctp_attestation::tests::attested(&operation);
            crate::cctp_attestation::validate_bound_message(&operation, &evidence, &message)
                .unwrap();
            assert_eq!(
                operation.cctp_transfer().unwrap().route.identity().unwrap(),
                transfer.route.identity().unwrap()
            );
            routes += 1;
        }
    }
    assert_eq!(routes, 42);
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
            5 => {
                route
                    .source
                    .network
                    .tokens
                    .iter_mut()
                    .find(|token| token.symbol == "USDC")
                    .unwrap()
                    .decimals = 18
            }
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
        (
            source.clone(),
            AssetIdentity::new("bnb", Some(t.route.destination.usdc)),
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
