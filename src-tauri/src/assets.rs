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
