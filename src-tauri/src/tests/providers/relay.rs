use super::*;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/relay-base-optimism.json")).unwrap()
}
fn fresh_fixture() -> Value {
    let mut body = fixture();
    let order = &mut body["protocol"]["v2"]["orderData"];
    order["output"]["deadline"] = json!(4102444800u64);
    for refund in order["inputs"][0]["refunds"].as_array_mut().unwrap() {
        refund["deadline"] = json!(4102444800u64);
    }
    let id = order_id(&body["protocol"]["v2"]["orderData"]).unwrap();
    body["protocol"]["v2"]["orderId"] = json!(id);
    let data = body["steps"][0]["items"][0]["data"]["data"]
        .as_str()
        .unwrap()
        .to_string();
    body["steps"][0]["items"][0]["data"]["data"] =
        json!(format!("{}{}", &data[..data.len() - 64], &id[2..]));
    body
}
fn parse(body: &Value) -> Result<(RelayData, u128, u128), String> {
    parse_quote(
        body,
        network_by_id("base").unwrap(),
        network_by_id("optimism").unwrap(),
        NATIVE,
        "0x0b2c639c533813f4aa9d7837caf62653d097ff85",
        "0x1111111111111111111111111111111111111111",
        10_000_000_000_000_000,
        50,
    )
}

#[test]
fn deposit_binds_every_reviewed_trade_field() {
    let (relay, expected, minimum) = parse(&fresh_fixture()).unwrap();
    let valid = |route: &RelayData, source, token_in, token_out, sender, amount, min, out| {
        deposit_call(route, source, token_in, token_out, sender, amount, min, out)
    };
    let source = "base";
    let token = NATIVE;
    let output = "0x0b2c639c533813f4aa9d7837caf62653d097ff85";
    let sender = "0x1111111111111111111111111111111111111111";
    let amount = 10_000_000_000_000_000;
    assert!(
        valid(
            &relay, source, token, output, sender, amount, minimum, expected
        )
        .is_ok()
    );
    assert!(
        valid(
            &relay, "ethereum", token, output, sender, amount, minimum, expected
        )
        .is_err()
    );
    assert!(
        valid(
            &relay, source, output, output, sender, amount, minimum, expected
        )
        .is_err()
    );
    assert!(
        valid(
            &relay, source, token, NATIVE, sender, amount, minimum, expected
        )
        .is_err()
    );
    assert!(
        valid(
            &relay, source, token, output, DEPOSITORY, amount, minimum, expected
        )
        .is_err()
    );
    assert!(
        valid(
            &relay,
            source,
            token,
            output,
            sender,
            amount + 1,
            minimum,
            expected
        )
        .is_err()
    );
    assert!(
        valid(
            &relay,
            source,
            token,
            output,
            sender,
            amount,
            minimum - 1,
            expected
        )
        .is_err()
    );
    assert!(
        valid(
            &relay,
            source,
            token,
            output,
            sender,
            amount,
            minimum,
            expected + 1
        )
        .is_err()
    );
    let mut corrupt = relay.clone();
    corrupt.order["output"]["payments"][0]["recipient"] = json!(DEPOSITORY);
    assert!(
        valid(
            &corrupt, source, token, output, sender, amount, minimum, expected
        )
        .is_err()
    );
    corrupt.order_id = order_id(&corrupt.order).unwrap();
    assert!(
        valid(
            &corrupt, source, token, output, sender, amount, minimum, expected
        )
        .is_err()
    );
}

#[test]
fn integer_rejects_float_and_exponent_amounts() {
    for value in [json!("1.2"), json!("1e18"), json!(-1), json!("-1")] {
        assert!(integer(&value).is_err());
    }
}

#[test]
fn order_hash_matches_live_relay_sdk_vector() {
    let body = fixture();
    assert_eq!(
        order_id(&body["protocol"]["v2"]["orderData"]).unwrap(),
        body["protocol"]["v2"]["orderId"].as_str().unwrap()
    );
    let (route, _, _) = parse(&body).unwrap();
    assert_eq!(route.route.len(), 2);
    assert!(route.route.iter().all(|n| n.pool_fee.is_none()));
}

#[test]
fn parser_rejects_calldata_target_and_slippage_tampering() {
    let body = fresh_fixture();
    for replacement in [json!("0x"), json!("0x49290c1c")] {
        let mut invalid = body.clone();
        invalid["steps"][0]["items"][0]["data"]["data"] = replacement;
        assert!(parse(&invalid).is_err());
    }
    let mut invalid = body.clone();
    invalid["steps"][0]["items"][0]["data"]["to"] = json!(SOLVER);
    assert!(parse(&invalid).is_err());
    let mut invalid = body.clone();
    invalid["protocol"]["v2"]["orderData"]["output"]["payments"][0]["minimumAmount"] = json!("1");
    assert!(parse(&invalid).is_err());
    let mut invalid = body.clone();
    invalid["steps"][0]["kind"] = json!("signature");
    assert!(parse(&invalid).is_err());
}

#[test]
fn settlement_requires_matching_receipt_token_recipient_and_minimum() {
    let (relay, _, minimum) = parse(&fresh_fixture()).unwrap();
    let intent = crate::uniswap::SwapIntent {
        api: None,
        network: network_by_id("base").unwrap().clone(),
        token_in: NATIVE.into(),
        token_out: "0x0b2c639c533813f4aa9d7837caf62653d097ff85".into(),
        sender: "0x1111111111111111111111111111111111111111".into(),
        amount: 10_000_000_000_000_000,
        relay: Some(relay.clone()),
    };
    let hash = format!("0x{}", "aa".repeat(32));
    let block = format!("0x{}", "bb".repeat(32));
    let mut observation = crate::tx::tx_status::TxStatus::new(
        crate::tx::tx_status::Execution::Succeeded,
        crate::tx::tx_status::Finality::Finalized,
    );
    observation.block_hash = Some(block.clone());
    let receipt = json!({"status":"0x1","transactionHash":hash,"blockHash":block,"logs":[{"address":intent.token_out,"topics":[format!("0x{}",hex::encode(Keccak256::digest("Transfer(address,address,uint256)"))),format!("0x{}","00".repeat(32)),format!("0x{}",hex::encode(evm_address_word(&intent.sender).unwrap()))],"data":format!("0x{minimum:064x}")}]});
    assert!(verify_destination_receipt(&intent, &relay, &hash, &observation, &receipt).is_ok());
    let mut self_transfer = receipt.clone();
    self_transfer["logs"][0]["topics"][1] = self_transfer["logs"][0]["topics"][2].clone();
    assert!(
        verify_destination_receipt(&intent, &relay, &hash, &observation, &self_transfer).is_err()
    );
    for pointer in [
        "/transactionHash",
        "/blockHash",
        "/logs/0/address",
        "/logs/0/topics/2",
    ] {
        let mut invalid = receipt.clone();
        *invalid.pointer_mut(pointer).unwrap() = json!(format!("0x{}", "cc".repeat(32)));
        assert!(
            verify_destination_receipt(&intent, &relay, &hash, &observation, &invalid).is_err()
        );
    }
    let mut invalid = receipt.clone();
    invalid["logs"][0]["data"] = json!(format!("0x{:064x}", minimum - 1));
    assert!(verify_destination_receipt(&intent, &relay, &hash, &observation, &invalid).is_err());
    let mut invalid = receipt.clone();
    invalid["logs"][0]["removed"] = json!(true);
    assert!(verify_destination_receipt(&intent, &relay, &hash, &observation, &invalid).is_err());
}

#[test]
fn verifies_solver_backed_same_chain_erc20_routes_without_a_v3_fee_restriction() {
    let body: Value =
        serde_json::from_str(include_str!("../fixtures/relay-base-usdc-uni.json")).unwrap();
    let source = network_by_id("base").unwrap();
    let input = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913";
    let output = "0xc3de830ea07524a0761646a6a4e4be0e114a3c83";
    let sender = "0x1111111111111111111111111111111111111111";
    let (relay, expected, minimum) =
        parse_quote(&body, source, source, input, output, sender, 10_000_000, 50).unwrap();
    assert_eq!(relay.route.len(), 2);
    let call = deposit_call(
        &relay, "base", input, output, sender, 10_000_000, minimum, expected,
    )
    .unwrap();
    assert_eq!(call.to, DEPOSITORY);
    assert_eq!(call.value, 0);
    assert_eq!(call.data.len(), 132);
    assert_eq!(order_id(&relay.order).unwrap(), relay.order_id);
    assert!(relay.route.iter().all(|n| n.pool_fee.is_none()));
}
