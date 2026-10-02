use crate::dto::{Asset, AssetIdentity};
use crate::registry::network_by_id;

/// Compares token identifiers case-insensitively on EVM and exactly elsewhere.
pub(crate) fn token_addresses_match(network_id: &str, left: &str, right: &str) -> bool {
    if network_by_id(network_id).is_some_and(|network| network.kind == "evm") {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

pub(crate) fn cached_asset(
    cached_assets: &[Asset],
    network_id: &str,
    token_address: Option<&str>,
) -> Option<Asset> {
    cached_assets
        .iter()
        .find(|asset| asset_matches(asset, network_id, token_address))
        .cloned()
}

pub(crate) fn cached_asset_by_token_address(
    cached_assets: &[Asset],
    network_id: &str,
    token_address: &str,
) -> Option<Asset> {
    cached_asset(cached_assets, network_id, Some(token_address))
}

pub(crate) fn asset_matches(asset: &Asset, network_id: &str, token_address: Option<&str>) -> bool {
    asset.network == network_id
        && match (asset.token_address.as_deref(), token_address) {
            (None, None) => true,
            (Some(left), Some(right)) => token_addresses_match(network_id, left, right),
            _ => false,
        }
}

impl AssetIdentity {
    pub(crate) fn new(network: &str, token_address: Option<&str>) -> Self {
        Self {
            network: network.to_string(),
            token_address: token_address.map(|address| {
                if network_by_id(network).is_some_and(|config| config.kind == "evm") {
                    address.to_ascii_lowercase()
                } else {
                    address.to_string()
                }
            }),
        }
    }

    pub(crate) fn matches(&self, asset: &Asset) -> bool {
        asset_matches(asset, &self.network, self.token_address.as_deref())
    }
}

#[cfg(test)]
#[path = "tests/assets.rs"]
mod tests;
