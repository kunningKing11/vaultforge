use super::deployment_for_network;

#[test]
fn base_swap_deployment_resolves_with_pinned_contracts() {
    let deployment = deployment_for_network("base").unwrap();
    assert_eq!(deployment.network_id, "base");
    assert_eq!(deployment.chain_id, 8453);
    assert_eq!(
        deployment.router,
        "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45"
    );
    assert_eq!(
        deployment.quoter_v2,
        "0x61ffe014BA17989A384D2f57319B24a1e383f4F6"
    );
    assert_eq!(deployment.routing_api_chain_param, "base");
}

#[test]
fn unknown_swap_networks_fail_closed() {
    for network in ["ethereum", "arbitrum_one", "solana", "monad", ""] {
        assert!(deployment_for_network(network).is_err());
    }
}
