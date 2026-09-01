use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NetworkRegistry {
    pub(crate) schema_version: u32,
    pub(crate) networks: Vec<NetworkConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NetworkConfig {
    pub(crate) kind: String,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) chain_id: Option<u64>,
    pub(crate) address_key: String,
    pub(crate) rpc_url: Option<String>,
    pub(crate) api_url: Option<String>,
    pub(crate) coin_gecko_network_id: Option<String>,
    pub(crate) native_asset: AssetConfig,
    #[serde(default)]
    pub(crate) tokens: Vec<AssetConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssetConfig {
    pub(crate) symbol: String,
    pub(crate) name: String,
    pub(crate) decimals: u32,
    pub(crate) coin_gecko_id: String,
    pub(crate) token_address: Option<String>,
}

impl NetworkConfig {
    pub(crate) fn rpc_url(&self) -> Result<&str, String> {
        self.rpc_url
            .as_deref()
            .ok_or_else(|| format!("{} RPC provider is not configured", self.name))
    }

    pub(crate) fn api_url(&self) -> Result<&str, String> {
        self.api_url
            .as_deref()
            .ok_or_else(|| format!("{} API provider is not configured", self.name))
    }

    pub(crate) fn chain_id(&self) -> Result<u64, String> {
        self.chain_id
            .ok_or_else(|| format!("{} chain ID is not configured", self.name))
    }
}

static REGISTRY: OnceLock<NetworkRegistry> = OnceLock::new();

pub(crate) fn registry() -> &'static NetworkRegistry {
    REGISTRY.get_or_init(|| {
        let registry: NetworkRegistry =
            serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/networks.json")))
                .expect("generated network registry must be valid JSON");
        assert_eq!(
            registry.schema_version, 1,
            "unsupported network registry schema"
        );
        registry
    })
}

pub(crate) fn network_by_id(network_id: &str) -> Option<&'static NetworkConfig> {
    registry()
        .networks
        .iter()
        .find(|network| network.id == network_id)
}

pub(crate) fn evm_networks() -> impl Iterator<Item = &'static NetworkConfig> {
    registry()
        .networks
        .iter()
        .filter(|network| network.kind == "evm")
}

pub(crate) fn configured_asset<'a>(
    network: &'a NetworkConfig,
    token_address: Option<&str>,
) -> Option<&'a AssetConfig> {
    match token_address {
        None => Some(&network.native_asset),
        Some(address) => network.tokens.iter().find(|token| {
            token
                .token_address
                .as_deref()
                .is_some_and(|configured| configured.eq_ignore_ascii_case(address))
        }),
    }
}

#[cfg(test)]
#[path = "tests/registry.rs"]
mod tests;
