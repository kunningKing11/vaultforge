use crate::dto::Asset;
use crate::registry::network_by_id;

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
    symbol: &str,
) -> Option<Asset> {
    cached_assets
        .iter()
        .find(|asset| asset.network == network_id && asset.symbol == symbol)
        .cloned()
}

pub(crate) fn cached_asset_by_token_address(
    cached_assets: &[Asset],
    network_id: &str,
    token_address: &str,
) -> Option<Asset> {
    cached_assets
        .iter()
        .find(|asset| {
            asset.network == network_id
                && asset.token_address.as_deref().is_some_and(|address| {
                    token_addresses_match(network_id, address, token_address)
                })
        })
        .cloned()
}

#[cfg(test)]
#[path = "tests/assets.rs"]
mod tests;
