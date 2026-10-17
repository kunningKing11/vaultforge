//! Verified Uniswap deployments for wallet swap chains.
//!
//! Single source of truth mapping the wallet network id to the chain's
//! audited Uniswap contracts. Seeded with one chain; adding a chain means
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

// Canonical Uniswap V3 SwapRouter02 and QuoterV2 deployments. Addresses are
// identical across EVM chains; each row is still pinned per chain so a
// registry mismatch fails closed instead of pointing at the wrong chain.
const DEPLOYMENTS: &[DeploymentSpec] = &[DeploymentSpec {
    id: "base",
    evm_chain_id: 8453,
    router: "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45",
    quoter_v2: "0x61ffe014BA17989A384D2f57319B24a1e383f4F6",
    routing_api_chain_param: "base",
}];

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
