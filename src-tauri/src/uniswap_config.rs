//! Verified Uniswap deployments for wallet swap chains.
//!
//! Single source of truth mapping the wallet network id to the chain's
//! verified Uniswap contracts. Adding a chain means
//! adding one verified row plus tests, never touching call sites.

use crate::address::evm::validate_address;
use crate::registry::{TransactionStatusProvider, network_by_id};

#[derive(Debug)]
pub(crate) struct UniswapDeployment {
    pub(crate) network_id: &'static str,
    pub(crate) chain_id: u64,
    pub(crate) router: &'static str,
    pub(crate) quoter_v2: &'static str,
    pub(crate) routing_api_chain_param: &'static str,
}

struct DeploymentSpec {
    id: &'static str,
    evm_chain_id: u64,
    router: &'static str,
    quoter_v2: &'static str,
    routing_api_chain_param: &'static str,
}

// Pinned from the official Uniswap deployment records:
// https://github.com/Uniswap/contracts/tree/main/deployments
// Chain IDs: 1, 10, 56, 137, 143, 8453, 42161, 43114. Never infer an address
// from another network, even when official deployments happen to coincide.
const DEPLOYMENTS: &[DeploymentSpec] = &[
    DeploymentSpec {
        id: "arbitrum_one",
        evm_chain_id: 42161,
        router: "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45",
        quoter_v2: "0x61ffe014ba17989e743c5f6cb21bf9697530b21e",
        routing_api_chain_param: "arbitrum",
    },
    DeploymentSpec {
        id: "avalanche_c",
        evm_chain_id: 43114,
        router: "0xbb00ff08d01d300023c629e8ffffcb65a5a578ce",
        quoter_v2: "0xbe0f5544ec67e9b3b2d979aaa43f18fd87e6257f",
        routing_api_chain_param: "avalanche",
    },
    DeploymentSpec {
        id: "base",
        evm_chain_id: 8453,
        router: "0x2626664c2603336e57b271c5c0b26f421741e481",
        quoter_v2: "0x3d4e44eb1374240ce5f1b871ab261cd16335b76a",
        routing_api_chain_param: "base",
    },
    DeploymentSpec {
        id: "bnb",
        evm_chain_id: 56,
        router: "0xb971ef87ede563556b2ed4b1c0b0019111dd85d2",
        quoter_v2: "0x78d78e420da98ad378d7799be8f4af69033eb077",
        routing_api_chain_param: "bnb",
    },
    DeploymentSpec {
        id: "ethereum",
        evm_chain_id: 1,
        router: "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45",
        quoter_v2: "0x61ffe014ba17989e743c5f6cb21bf9697530b21e",
        routing_api_chain_param: "ethereum",
    },
    DeploymentSpec {
        id: "monad",
        evm_chain_id: 143,
        router: "0xfe31f71c1b106eac32f1a19239c9a9a72ddfb900",
        quoter_v2: "0x661e93cca42afacb172121ef892830ca3b70f08d",
        routing_api_chain_param: "monad",
    },
    DeploymentSpec {
        id: "optimism",
        evm_chain_id: 10,
        router: "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45",
        quoter_v2: "0x61ffe014ba17989e743c5f6cb21bf9697530b21e",
        routing_api_chain_param: "optimism",
    },
    DeploymentSpec {
        id: "polygon",
        evm_chain_id: 137,
        router: "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45",
        quoter_v2: "0x61ffe014ba17989e743c5f6cb21bf9697530b21e",
        routing_api_chain_param: "polygon",
    },
];

/// Returns the verified Uniswap deployment for a wallet network id.
/// Fails closed for unknown networks and for registry drift (wrong kind,
/// chain id, or status provider).
pub(crate) fn deployment_for_network(network_id: &str) -> Result<UniswapDeployment, String> {
    let spec = DEPLOYMENTS
        .iter()
        .find(|deployment| deployment.id == network_id)
        .ok_or_else(|| format!("Unsupported swap network {network_id}"))?;
    let network = network_by_id(spec.id).ok_or("Swap wallet network is missing")?;
    if network.kind != "evm"
        || network.chain_id != Some(spec.evm_chain_id)
        || network.address_key != "evm"
        || network.transaction_status_provider != Some(TransactionStatusProvider::Evm)
    {
        return Err("Unsupported swap deployment or registry drift".into());
    }
    for address in [spec.router, spec.quoter_v2] {
        validate_address(address)?;
    }
    let url = reqwest::Url::parse(network.rpc_url()?).map_err(|_| "Invalid swap RPC URL")?;
    let secure = url.scheme() == "https" && url.host_str().is_some();
    #[cfg(test)]
    let secure = secure || (url.scheme() == "http" && url.host_str() == Some("127.0.0.1"));
    if !secure {
        return Err("Swap RPC must use HTTPS".into());
    }
    Ok(UniswapDeployment {
        network_id: spec.id,
        chain_id: spec.evm_chain_id,
        router: spec.router,
        quoter_v2: spec.quoter_v2,
        routing_api_chain_param: spec.routing_api_chain_param,
    })
}

#[cfg(test)]
#[path = "tests/uniswap_config.rs"]
pub(crate) mod tests;
