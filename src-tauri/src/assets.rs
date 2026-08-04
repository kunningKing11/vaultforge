use crate::dto::Asset;

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
                && asset
                    .token_address
                    .as_deref()
                    .is_some_and(|address| address.eq_ignore_ascii_case(token_address))
        })
        .cloned()
}
