use super::available_routes;
use std::collections::HashMap;

#[test]
fn solana_routes_are_executable_in_both_directions() {
    let addresses = HashMap::from([
        ("evm".to_string(), "evm-address".to_string()),
        ("solana".to_string(), "solana-address".to_string()),
    ]);
    let routes = available_routes(&addresses);
    for (source, destination) in [("ethereum", "solana"), ("solana", "ethereum")] {
        let route = routes
            .iter()
            .find(|route| route.source == source && route.destination == destination)
            .expect("expected CCTP route");
        assert!(route.executable);
    }
}
