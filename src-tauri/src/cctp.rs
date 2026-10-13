//! Validated CCTP V2 Standard deployments for wallet EVM chains and Solana.

use serde::{Deserialize, Serialize};

use crate::address::evm::validate_address;
use crate::dto::AssetIdentity;
use crate::registry::{NetworkConfig, TransactionStatusProvider, configured_asset, network_by_id};
use crate::tx::tx_status::Finality;

pub(crate) const CCTP_PROTOCOL_VERSION: u32 = 2;
// Native construction/recovery is covered by mocks; enable execution only after
// a funded native-route integration proves the pinned deployment and restart.
pub(crate) const NATIVE_EXECUTION_ENABLED: bool = false;

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
    pub(crate) source_chain_id: Option<u64>,
    pub(crate) destination_chain_id: Option<u64>,
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

struct DeploymentSpec {
    id: &'static str,
    evm_chain_id: Option<u64>,
    domain: u32,
    symbol: &'static str,
    usdc: &'static str,
    environment: CctpEnvironment,
}

// Circle native USDC and V2 domains, restricted to implemented wallet EVM chains.
const MAINNET_DEPLOYMENTS: &[DeploymentSpec] = &[
    DeploymentSpec {
        id: "ethereum",
        evm_chain_id: Some(1),
        domain: 0,
        symbol: "ETH",
        usdc: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
        environment: CctpEnvironment::Mainnet,
    },
    DeploymentSpec {
        id: "avalanche_c",
        evm_chain_id: Some(43114),
        domain: 1,
        symbol: "AVAX",
        usdc: "0xB97EF9Ef8734C71904D8002F8b6Bc66Dd9c48a6E",
        environment: CctpEnvironment::Mainnet,
    },
    DeploymentSpec {
        id: "optimism",
        evm_chain_id: Some(10),
        domain: 2,
        symbol: "ETH",
        usdc: "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85",
        environment: CctpEnvironment::Mainnet,
    },
    DeploymentSpec {
        id: "arbitrum_one",
        evm_chain_id: Some(42161),
        domain: 3,
        symbol: "ETH",
        usdc: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
        environment: CctpEnvironment::Mainnet,
    },
    DeploymentSpec {
        id: "base",
        evm_chain_id: Some(8453),
        domain: 6,
        symbol: "ETH",
        usdc: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
        environment: CctpEnvironment::Mainnet,
    },
    DeploymentSpec {
        id: "polygon",
        evm_chain_id: Some(137),
        domain: 7,
        symbol: "POL",
        usdc: "0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359",
        environment: CctpEnvironment::Mainnet,
    },
    DeploymentSpec {
        id: "monad",
        evm_chain_id: Some(143),
        domain: 15,
        symbol: "MON",
        usdc: "0x754704Bc059F8C67012fEd69BC8A327a5aafb603",
        environment: CctpEnvironment::Mainnet,
    },
];
const SOLANA_DEPLOYMENT: DeploymentSpec = DeploymentSpec {
    id: "solana",
    evm_chain_id: None,
    domain: 5,
    symbol: "SOL",
    usdc: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    environment: CctpEnvironment::Mainnet,
};
pub(crate) const SOLANA_MESSENGER: &str = "CCTPV2vPZJS2u2BBsUoscuikbYjnpFmbFsvVuJdgUMQe";
pub(crate) const SOLANA_TRANSMITTER: &str = "CCTPV2Sm4AdWt5296sk4P66VBZ7bEhcARwFaaS9YPbeC";

const SEPOLIA_DEPLOYMENTS: &[DeploymentSpec] = &[
    DeploymentSpec {
        id: "ethereum_sepolia",
        evm_chain_id: Some(11155111),
        domain: 0,
        symbol: "ETH",
        usdc: "0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238",
        environment: CctpEnvironment::Sepolia,
    },
    DeploymentSpec {
        id: "arbitrum_sepolia",
        evm_chain_id: Some(421614),
        domain: 3,
        symbol: "ETH",
        usdc: "0x75faf114eafb1BDbe2F0316DF893fd58CE46AA4d",
        environment: CctpEnvironment::Sepolia,
    },
];

fn deployment_spec(id: &str) -> Result<&'static DeploymentSpec, String> {
    if id == "solana" {
        return Ok(&SOLANA_DEPLOYMENT);
    }
    MAINNET_DEPLOYMENTS
        .iter()
        .chain(SEPOLIA_DEPLOYMENTS)
        .find(|deployment| deployment.id == id)
        .ok_or_else(|| format!("Unsupported CCTP wallet deployment {id}"))
}

fn contracts(environment: CctpEnvironment) -> (&'static str, &'static str, &'static str) {
    match environment {
        CctpEnvironment::Mainnet => (
            "0x28b5a0e9C621a5BadaA536219b3a228C8168cf5d",
            "0x81D40F21F12A8F0E3252Bccb954D722d4c464B64",
            "https://iris-api.circle.com",
        ),
        CctpEnvironment::Sepolia => (
            "0x8FE6B999Dc680CcFDD5Bf7EB0974218be2542DAA",
            "0xE737e5cEBEEBa77EFE34D4aa090756590b1CE275",
            "https://iris-api-sandbox.circle.com",
        ),
    }
}

fn deployment(spec: &DeploymentSpec) -> Result<CctpDeployment, String> {
    let network = match spec.environment {
        CctpEnvironment::Mainnet => network_by_id(spec.id)
            .ok_or("CCTP wallet network is missing")?
            .clone(),
        CctpEnvironment::Sepolia => match spec.id {
            "ethereum_sepolia" => testnet_network(
                spec.id,
                "Ethereum Sepolia",
                spec.evm_chain_id.ok_or("Missing CCTP testnet chain ID")?,
                "https://ethereum-sepolia-rpc.publicnode.com",
                spec.usdc,
            )?,
            "arbitrum_sepolia" => testnet_network(
                spec.id,
                "Arbitrum Sepolia",
                spec.evm_chain_id.ok_or("Missing CCTP testnet chain ID")?,
                "https://sepolia-rollup.arbitrum.io/rpc",
                spec.usdc,
            )?,
            _ => return Err("Unsupported CCTP testnet".into()),
        },
    };
    let (messenger, transmitter, _) = contracts(spec.environment);
    let (messenger, transmitter) = if spec.id == "solana" {
        (SOLANA_MESSENGER, SOLANA_TRANSMITTER)
    } else {
        (messenger, transmitter)
    };
    Ok(CctpDeployment {
        network,
        domain: spec.domain,
        usdc: spec.usdc,
        token_messenger: messenger,
        message_transmitter: transmitter,
    })
}

impl CctpRoute {
    /// Compatibility constructor for the original route and its test counterpart.
    pub(crate) fn new(environment: CctpEnvironment) -> Result<Self, String> {
        match environment {
            CctpEnvironment::Mainnet => Self::for_networks("ethereum", "arbitrum_one"),
            CctpEnvironment::Sepolia => Self::for_networks("ethereum_sepolia", "arbitrum_sepolia"),
        }
    }

    pub(crate) fn for_networks(source: &str, destination: &str) -> Result<Self, String> {
        let source = deployment_spec(source)?;
        let destination = deployment_spec(destination)?;
        if source.id == destination.id || source.environment != destination.environment {
            return Err("CCTP requires different chains in the same environment".into());
        }
        let route = Self {
            protocol_version: CCTP_PROTOCOL_VERSION,
            environment: source.environment,
            source: deployment(source)?,
            destination: deployment(destination)?,
            attestation_host: contracts(source.environment).2,
        };
        route.validate()?;
        Ok(route)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let (messenger, transmitter, host) = contracts(self.environment);
        if self.protocol_version != CCTP_PROTOCOL_VERSION
            || self.attestation_host != host
            || self.source.network.id == self.destination.network.id
        {
            return Err("Unsupported CCTP protocol, environment, or same-chain route".into());
        }
        for deployment in [&self.source, &self.destination] {
            let spec = deployment_spec(&deployment.network.id)?;
            let network = &deployment.network;
            let solana = spec.id == "solana";
            let (expected_messenger, expected_transmitter) = if solana {
                (SOLANA_MESSENGER, SOLANA_TRANSMITTER)
            } else {
                (messenger, transmitter)
            };
            if spec.environment != self.environment
                || network.kind != if solana { "svm" } else { "evm" }
                || network.chain_id != spec.evm_chain_id
                || network.address_key != if solana { "solana" } else { "evm" }
                || network.transaction_status_provider
                    != Some(if solana {
                        TransactionStatusProvider::Solana
                    } else {
                        TransactionStatusProvider::Evm
                    })
                || network.native_asset.decimals != if solana { 9 } else { 18 }
                || network.native_asset.symbol != spec.symbol
                || deployment.domain != spec.domain
                || deployment.usdc != spec.usdc
                || deployment.token_messenger != expected_messenger
                || deployment.message_transmitter != expected_transmitter
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
                if solana {
                    crate::address::solana::validate_address(address)?;
                } else {
                    validate_address(address)?;
                }
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
            source_chain_id: self.source.network.chain_id,
            destination_chain_id: self.destination.network.chain_id,
            source_domain: self.source.domain,
            destination_domain: self.destination.domain,
            source_usdc: if self.source.network.kind == "evm" {
                self.source.usdc.to_ascii_lowercase()
            } else {
                self.source.usdc.into()
            },
            destination_usdc: if self.destination.network.kind == "evm" {
                self.destination.usdc.to_ascii_lowercase()
            } else {
                self.destination.usdc.into()
            },
            token_messenger: if self.source.network.kind == "evm" {
                self.source.token_messenger.to_ascii_lowercase()
            } else {
                self.source.token_messenger.into()
            },
            destination_token_messenger: if self.destination.network.kind == "evm" {
                self.destination.token_messenger.to_ascii_lowercase()
            } else {
                self.destination.token_messenger.into()
            },
            source_message_transmitter: if self.source.network.kind == "evm" {
                self.source.message_transmitter.to_ascii_lowercase()
            } else {
                self.source.message_transmitter.into()
            },
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

/// EVM routes bind the source wallet as mint caller; cross-family routes permit relayers.
/// zero maxFee rejects any protocol fee rather than silently changing net receipt.
#[derive(Debug)]
pub(crate) struct CctpTransfer {
    pub(crate) route: CctpRoute,
    sender: String,
    recipient: String,
    amount: u128,
    fee_payer: Option<String>,
}

impl CctpTransfer {
    pub(crate) fn new(
        source: &AssetIdentity,
        destination: &AssetIdentity,
        sender: &str,
        recipient: &str,
        amount: u128,
    ) -> Result<Self, String> {
        let route = CctpRoute::for_networks(&source.network, &destination.network)?;
        for (asset, deployment) in [(source, &route.source), (destination, &route.destination)] {
            if !asset.token_address.as_deref().is_some_and(|token| {
                if deployment.network.kind == "evm" {
                    token.eq_ignore_ascii_case(deployment.usdc)
                } else {
                    token == deployment.usdc
                }
            }) {
                return Err("CCTP route supports only Circle-issued native USDC".into());
            }
        }
        if route.source.network.kind == "evm" {
            validate_address(sender)?;
        } else {
            crate::address::solana::validate_address(sender)?;
        }
        if route.destination.network.kind == "evm" {
            validate_address(recipient)?;
        } else {
            crate::address::solana::validate_address(recipient)?;
        }
        if address_word(&route.source.network, sender)? == [0; 32]
            || address_word(&route.destination.network, recipient)? == [0; 32]
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
            fee_payer: None,
        })
    }
    pub(crate) fn with_destination_fee_payer(
        mut self,
        address: Option<&str>,
    ) -> Result<Self, String> {
        if let Some(address) = address {
            address_word(&self.route.destination.network, address)?;
            if self.route.source.network.kind == "evm"
                && self.route.destination.network.kind == "evm"
                && !address.eq_ignore_ascii_case(&self.sender)
            {
                return Err("Destination payer differs from bound caller".into());
            }
            self.fee_payer = Some(address.into());
        }
        Ok(self)
    }
    pub(crate) fn destination_fee_payer(&self) -> Result<&str, String> {
        self.fee_payer
            .as_deref()
            .or_else(|| {
                (self.route.source.network.kind == "evm"
                    && self.route.destination.network.kind == "evm")
                    .then_some(self.sender.as_str())
            })
            .ok_or("Destination wallet fee payer is missing".into())
    }
    pub(crate) fn sender(&self) -> &str {
        &self.sender
    }
    pub(crate) fn recipient(&self) -> &str {
        &self.recipient
    }
    pub(crate) fn destination_caller(&self) -> &str {
        if self.route.source.network.kind == "evm" && self.route.destination.network.kind == "evm" {
            &self.sender
        } else if self.route.destination.network.kind == "evm" {
            "0x0000000000000000000000000000000000000000"
        } else {
            "11111111111111111111111111111111"
        }
    }
    pub(crate) fn source_hash_matches(&self, left: &str, right: &str) -> bool {
        if self.route.source.network.kind == "evm" {
            left.eq_ignore_ascii_case(right)
        } else {
            left == right
        }
    }
    pub(crate) fn mint_recipient_word(&self) -> Result<[u8; 32], String> {
        if self.route.destination.network.kind == "svm" {
            address_word(
                &self.route.destination.network,
                &crate::tx::solana::solana_associated_token_address(
                    self.recipient(),
                    self.route.destination.usdc,
                )?,
            )
        } else {
            address_word(&self.route.destination.network, self.recipient())
        }
    }
    pub(crate) fn caller_word(&self) -> Result<[u8; 32], String> {
        address_word(&self.route.destination.network, self.destination_caller())
    }
    pub(crate) fn amount(&self) -> u128 {
        self.amount
    }
}

pub(crate) fn address_word(network: &NetworkConfig, address: &str) -> Result<[u8; 32], String> {
    if network.kind == "evm" {
        crate::tx::evm::evm_address_word(address)
    } else if network.kind == "svm" {
        use std::str::FromStr;
        solana_pubkey::Pubkey::from_str(address)
            .map(|key| key.to_bytes())
            .map_err(|_| "Invalid Solana CCTP address".into())
    } else {
        Err("Unsupported CCTP address family".into())
    }
}

#[cfg(test)]
#[path = "tests/cctp.rs"]
pub(crate) mod tests;
