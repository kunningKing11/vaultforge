use super::get_provider;

#[test]
fn provider_trait_covers_all_chains() {
    for symbol in &["ETH", "BTC", "SOL", "ZEC", "FIL", "INJ", "MATIC"] {
        let provider = get_provider(symbol);
        assert!(provider.is_some(), "No provider for symbol {symbol}");
    }
}
