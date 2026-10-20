use super::deployment_for_network;

#[test]
fn base_swap_deployment_resolves_with_pinned_contracts() {
    let deployment = deployment_for_network("base").unwrap();
    assert_eq!(deployment.network_id, "base");
    assert_eq!(deployment.chain_id, 8453);
    assert_eq!(
        deployment.router,
        "0x2626664c2603336e57b271c5c0b26f421741e481"
    );
    assert_eq!(
        deployment.quoter_v2,
        "0x3d4e44eb1374240ce5f1b871ab261cd16335b76a"
    );
    assert_eq!(deployment.routing_api_chain_param, "base");
}

#[test]
fn supported_evm_deployments_resolve_against_the_wallet_registry() {
    {
        let d = deployment_for_network("arbitrum_one").unwrap();
        assert_eq!(d.chain_id, 42161);
        assert_eq!(d.router, "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45");
        assert_eq!(d.quoter_v2, "0x61ffe014ba17989e743c5f6cb21bf9697530b21e");
    }
    {
        let d = deployment_for_network("avalanche_c").unwrap();
        assert_eq!(d.chain_id, 43114);
        assert_eq!(d.router, "0xbb00ff08d01d300023c629e8ffffcb65a5a578ce");
        assert_eq!(d.quoter_v2, "0xbe0f5544ec67e9b3b2d979aaa43f18fd87e6257f");
    }
    {
        let d = deployment_for_network("base").unwrap();
        assert_eq!(d.chain_id, 8453);
        assert_eq!(d.router, "0x2626664c2603336e57b271c5c0b26f421741e481");
        assert_eq!(d.quoter_v2, "0x3d4e44eb1374240ce5f1b871ab261cd16335b76a");
    }
    {
        let d = deployment_for_network("bnb").unwrap();
        assert_eq!(d.chain_id, 56);
        assert_eq!(d.router, "0xb971ef87ede563556b2ed4b1c0b0019111dd85d2");
        assert_eq!(d.quoter_v2, "0x78d78e420da98ad378d7799be8f4af69033eb077");
    }
    {
        let d = deployment_for_network("ethereum").unwrap();
        assert_eq!(d.chain_id, 1);
        assert_eq!(d.router, "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45");
        assert_eq!(d.quoter_v2, "0x61ffe014ba17989e743c5f6cb21bf9697530b21e");
    }
    {
        let d = deployment_for_network("monad").unwrap();
        assert_eq!(d.chain_id, 143);
        assert_eq!(d.router, "0xfe31f71c1b106eac32f1a19239c9a9a72ddfb900");
        assert_eq!(d.quoter_v2, "0x661e93cca42afacb172121ef892830ca3b70f08d");
    }
    {
        let d = deployment_for_network("optimism").unwrap();
        assert_eq!(d.chain_id, 10);
        assert_eq!(d.router, "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45");
        assert_eq!(d.quoter_v2, "0x61ffe014ba17989e743c5f6cb21bf9697530b21e");
    }
    {
        let d = deployment_for_network("polygon").unwrap();
        assert_eq!(d.chain_id, 137);
        assert_eq!(d.router, "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45");
        assert_eq!(d.quoter_v2, "0x61ffe014ba17989e743c5f6cb21bf9697530b21e");
    }
}

#[test]
fn unknown_swap_networks_fail_closed() {
    for network in ["bitcoin", "solana", "tron", "xrpl", "ethereum_sepolia", ""] {
        assert!(deployment_for_network(network).is_err());
    }
}
