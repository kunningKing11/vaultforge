//! Fixed CCTP V2 Standard Transfer deployment. No signing or execution commands.

use serde::{Deserialize, Serialize};

use crate::address::evm::validate_address;
use crate::dto::AssetIdentity;
use crate::registry::{NetworkConfig, TransactionStatusProvider, configured_asset, network_by_id};
use crate::tx::tx_status::Finality;

pub(crate) const CCTP_PROTOCOL_VERSION: u32 = 2;
pub(crate) const STANDARD_FINALITY_THRESHOLD: u32 = 2000;
pub(crate) const MAX_BURN_AMOUNT: u128 = 10_000_000 * 1_000_000;
// Provisional budgets, not RPC estimates or guaranteed upper bounds. Every real call
// must be estimated again before signing, including Arbitrum's L1 data cost.
pub(crate) const BURN_GAS_BUDGET: u64 = 200_000;
pub(crate) const MINT_GAS_BUDGET: u64 = 300_000;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CctpEnvironment {
    Mainnet,
    Sepolia,
}

#[derive(Debug)]
pub(crate) struct CctpDeployment {
    pub(crate) network: NetworkConfig,
    pub(crate) domain: u32,
    pub(crate) usdc: &'static str,
    pub(crate) token_messenger: &'static str,
    pub(crate) message_transmitter: &'static str,
}

#[derive(Debug)]
pub(crate) struct CctpRoute {
    pub(crate) protocol_version: u32,
    pub(crate) environment: CctpEnvironment,
    pub(crate) source: CctpDeployment,
    pub(crate) destination: CctpDeployment,
    pub(crate) attestation_host: &'static str,
}

/// Encrypted deployment binding retained across restarts and configuration changes.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct CctpDeploymentIdentity {
    pub(crate) environment: CctpEnvironment,
    pub(crate) protocol_version: u32,
    pub(crate) source_chain_id: u64,
    pub(crate) destination_chain_id: u64,
    pub(crate) source_domain: u32,
    pub(crate) destination_domain: u32,
    pub(crate) source_usdc: String,
    pub(crate) destination_usdc: String,
    pub(crate) token_messenger: String,
    pub(crate) destination_token_messenger: String,
    pub(crate) source_message_transmitter: String,
    pub(crate) destination_message_transmitter: String,
    pub(crate) attestation_host: String,
}

struct RouteSpec {
    source_id: &'static str,
    source_chain: u64,
    destination_id: &'static str,
    destination_chain: u64,
    source_usdc: &'static str,
    destination_usdc: &'static str,
    messenger: &'static str,
    transmitter: &'static str,
    host: &'static str,
}

fn route_spec(environment: CctpEnvironment) -> RouteSpec {
    match environment {
        CctpEnvironment::Mainnet => RouteSpec {
            source_id: "ethereum",
            source_chain: 1,
            destination_id: "arbitrum_one",
            destination_chain: 42161,
            source_usdc: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            destination_usdc: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
            messenger: "0x28b5a0e9C621a5BadaA536219b3a228C8168cf5d",
            transmitter: "0x81D40F21F12A8F0E3252Bccb954D722d4c464B64",
            host: "https://iris-api.circle.com",
        },
        CctpEnvironment::Sepolia => RouteSpec {
            source_id: "ethereum_sepolia",
            source_chain: 11155111,
            destination_id: "arbitrum_sepolia",
            destination_chain: 421614,
            source_usdc: "0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238",
            destination_usdc: "0x75faf114eafb1BDbe2F0316DF893fd58CE46AA4d",
            messenger: "0x8FE6B999Dc680CcFDD5Bf7EB0974218be2542DAA",
            transmitter: "0xE737e5cEBEEBa77EFE34D4aa090756590b1CE275",
            host: "https://iris-api-sandbox.circle.com",
        },
    }
}

impl CctpRoute {
    /// Circle deployment constants: contract-addresses and usdc-contract-addresses.
    /// Version is deliberately fixed to V2; domains are not EVM chain IDs.
    pub(crate) fn new(environment: CctpEnvironment) -> Result<Self, String> {
        let spec = route_spec(environment);
        let (source, destination) = match environment {
            CctpEnvironment::Mainnet => (
                network_by_id(spec.source_id)
                    .ok_or("Ethereum is not configured")?
                    .clone(),
                network_by_id(spec.destination_id)
                    .ok_or("Arbitrum One is not configured")?
                    .clone(),
            ),
            CctpEnvironment::Sepolia => (
                testnet_network(
                    spec.source_id,
                    "Ethereum Sepolia",
                    spec.source_chain,
                    "https://ethereum-sepolia-rpc.publicnode.com",
                    spec.source_usdc,
                )?,
                testnet_network(
                    spec.destination_id,
                    "Arbitrum Sepolia",
                    spec.destination_chain,
                    "https://sepolia-rollup.arbitrum.io/rpc",
                    spec.destination_usdc,
                )?,
            ),
        };
        let route = Self {
            protocol_version: CCTP_PROTOCOL_VERSION,
            environment,
            source: CctpDeployment {
                network: source,
                domain: 0,
                usdc: spec.source_usdc,
                token_messenger: spec.messenger,
                message_transmitter: spec.transmitter,
            },
            destination: CctpDeployment {
                network: destination,
                domain: 3,
                usdc: spec.destination_usdc,
                token_messenger: spec.messenger,
                message_transmitter: spec.transmitter,
            },
            attestation_host: spec.host,
        };
        route.validate()?;
        Ok(route)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let spec = route_spec(self.environment);
        if self.protocol_version != CCTP_PROTOCOL_VERSION {
            return Err("Unsupported CCTP protocol version".into());
        }
        if self.attestation_host != spec.host {
            return Err("CCTP attestation environment mismatch".into());
        }
        for (deployment, id, chain, domain, usdc) in [
            (
                &self.source,
                spec.source_id,
                spec.source_chain,
                0,
                spec.source_usdc,
            ),
            (
                &self.destination,
                spec.destination_id,
                spec.destination_chain,
                3,
                spec.destination_usdc,
            ),
        ] {
            let network = &deployment.network;
            if network.id != id
                || network.kind != "evm"
                || network.chain_id()? != chain
                || network.address_key != "evm"
                || network.transaction_status_provider != Some(TransactionStatusProvider::Evm)
                || network.native_asset.decimals != 18
                || network.native_asset.symbol != "ETH"
                || deployment.domain != domain
                || !deployment.usdc.eq_ignore_ascii_case(usdc)
                || !deployment
                    .token_messenger
                    .eq_ignore_ascii_case(spec.messenger)
                || !deployment
                    .message_transmitter
                    .eq_ignore_ascii_case(spec.transmitter)
            {
                return Err("Unsupported CCTP deployment or mixed environments".into());
            }
            let token = configured_asset(network, Some(deployment.usdc))
                .ok_or("CCTP native USDC is not configured")?;
            if token.decimals != 6 {
                return Err("CCTP USDC must use six decimals".into());
            }
            for address in [
                deployment.usdc,
                deployment.token_messenger,
                deployment.message_transmitter,
            ] {
                validate_address(address)?;
            }
            let url =
                reqwest::Url::parse(network.rpc_url()?).map_err(|_| "Invalid CCTP RPC URL")?;
            let secure = url.scheme() == "https" && url.host_str().is_some();
            #[cfg(test)]
            let secure = secure || (url.scheme() == "http" && url.host_str() == Some("127.0.0.1"));
            if !secure {
                return Err("CCTP RPC must use HTTPS".into());
            }
        }
        Ok(())
    }

    pub(crate) fn identity(&self) -> Result<CctpDeploymentIdentity, String> {
        self.validate()?;
        Ok(CctpDeploymentIdentity {
            environment: self.environment,
            protocol_version: self.protocol_version,
            source_chain_id: self.source.network.chain_id()?,
            destination_chain_id: self.destination.network.chain_id()?,
            source_domain: self.source.domain,
            destination_domain: self.destination.domain,
            source_usdc: self.source.usdc.to_ascii_lowercase(),
            destination_usdc: self.destination.usdc.to_ascii_lowercase(),
            token_messenger: self.source.token_messenger.to_ascii_lowercase(),
            destination_token_messenger: self.destination.token_messenger.to_ascii_lowercase(),
            source_message_transmitter: self.source.message_transmitter.to_ascii_lowercase(),
            destination_message_transmitter: self
                .destination
                .message_transmitter
                .to_ascii_lowercase(),
            attestation_host: self.attestation_host.into(),
        })
    }

    pub(crate) fn required_source_finality(&self) -> Finality {
        Finality::Finalized
    }
}

fn testnet_network(
    id: &str,
    name: &str,
    chain: u64,
    rpc: &str,
    usdc: &str,
) -> Result<NetworkConfig, String> {
    // These endpoints are private to CCTP preparation, not enabled wallet networks.
    let mut network = network_by_id("ethereum")
        .ok_or("Ethereum is not configured")?
        .clone();
    let mut token = configured_asset(&network, Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"))
        .ok_or("Ethereum USDC is not configured")?
        .clone();
    token.token_address = Some(usdc.into());
    network.id = id.into();
    network.name = name.into();
    network.chain_id = Some(chain);
    network.rpc_url = Some(rpc.into());
    network.tokens = vec![token];
    Ok(network)
}

/// A validated request. The source wallet is also the destination mint caller;
/// zero maxFee rejects any protocol fee rather than silently changing net receipt.
#[derive(Debug)]
pub(crate) struct CctpTransfer {
    pub(crate) route: CctpRoute,
    sender: String,
    recipient: String,
    amount: u128,
}

impl CctpTransfer {
    pub(crate) fn new(
        source: &AssetIdentity,
        destination: &AssetIdentity,
        sender: &str,
        recipient: &str,
        amount: u128,
    ) -> Result<Self, String> {
        let environment = match (source.network.as_str(), destination.network.as_str()) {
            ("ethereum", "arbitrum_one") => CctpEnvironment::Mainnet,
            ("ethereum_sepolia", "arbitrum_sepolia") => CctpEnvironment::Sepolia,
            _ => return Err("Unsupported CCTP route".into()),
        };
        let route = CctpRoute::new(environment)?;
        for (asset, deployment) in [(source, &route.source), (destination, &route.destination)] {
            if !asset
                .token_address
                .as_deref()
                .is_some_and(|token| token.eq_ignore_ascii_case(deployment.usdc))
            {
                return Err("CCTP route supports only Circle-issued native USDC".into());
            }
        }
        validate_address(sender)?;
        validate_address(recipient)?;
        if sender[2..].bytes().all(|byte| byte == b'0')
            || recipient[2..].bytes().all(|byte| byte == b'0')
        {
            return Err("CCTP accounts cannot be zero addresses".into());
        }
        if amount == 0 || amount > MAX_BURN_AMOUNT {
            return Err("CCTP amount must be positive and at most 10 million USDC".into());
        }
        Ok(Self {
            route,
            sender: sender.into(),
            recipient: recipient.into(),
            amount,
        })
    }
    pub(crate) fn sender(&self) -> &str {
        &self.sender
    }
    pub(crate) fn recipient(&self) -> &str {
        &self.recipient
    }
    pub(crate) fn destination_caller(&self) -> &str {
        &self.sender
    }
    pub(crate) fn amount(&self) -> u128 {
        self.amount
    }
}

#[cfg(test)]
#[path = "tests/cctp.rs"]
pub(crate) mod tests;
