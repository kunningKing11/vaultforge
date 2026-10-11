use super::{bitcoin, evm, get_provider, solana, tron, tx_observation_provider, xrpl};
use crate::registry::{NetworkRegistry, TransactionStatusProvider, registry};
use std::time::Duration;

#[test]
fn provider_trait_covers_all_chains() {
    for symbol in &["ETH", "BTC", "SOL", "ZEC", "FIL", "INJ", "MATIC"] {
        let provider = get_provider(symbol);
        assert!(provider.is_some(), "No provider for symbol {symbol}");
    }
}

#[test]
fn transaction_status_lookups_use_the_supplied_network_endpoint() {
    let source: NetworkRegistry =
        serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/networks.json"))).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();

    for mut config in source.networks {
        let Some(provider) = config.transaction_status_provider else {
            continue;
        };
        config.name = format!("{} endpoint fixture", config.id);
        config.rpc_url = None;
        config.api_url = None;
        let error = runtime
            .block_on(async {
                match provider {
                    TransactionStatusProvider::Bitcoin => {
                        bitcoin::fetch_bitcoin_tx_observation(&client, &config, "hash").await
                    }
                    TransactionStatusProvider::Evm => {
                        evm::fetch_evm_tx_observation(&client, &config, "hash").await
                    }
                    TransactionStatusProvider::Solana => {
                        solana::fetch_solana_tx_observation(&client, &config, "hash").await
                    }
                    TransactionStatusProvider::Tron => {
                        tron::fetch_tron_tx_observation(&client, &config, "hash").await
                    }
                    TransactionStatusProvider::Xrpl => {
                        xrpl::fetch_xrpl_tx_observation(&client, &config, "hash").await
                    }
                }
            })
            .unwrap_err();
        let endpoint = if provider == TransactionStatusProvider::Bitcoin {
            "API"
        } else {
            "RPC"
        };
        assert_eq!(
            error,
            format!("{} {endpoint} provider is not configured", config.name)
        );
    }
}

#[test]
fn transaction_status_routes_follow_registry_capabilities() {
    for network in &registry().networks {
        let route = tx_observation_provider(&network.id);
        match network.transaction_status_provider {
            Some(expected) => {
                let (config, provider) = route.unwrap();
                assert!(std::ptr::eq(config, network));
                assert_eq!(provider, expected);
            }
            None => assert_eq!(
                route.unwrap_err(),
                format!(
                    "Transaction status provider is unavailable for {}",
                    network.id
                )
            ),
        }
    }
    assert_eq!(
        tx_observation_provider("unsupported").unwrap_err(),
        "Unsupported network unsupported"
    );
}
