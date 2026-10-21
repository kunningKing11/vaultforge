use super::*;
use crate::tx::evm::evm_uint_word;
const SENDER: &str = "0x3333333333333333333333333333333333333333";
const INPUT: &str = "0x1111111111111111111111111111111111111111";
const OUTPUT: &str = "0x2222222222222222222222222222222222222222";

pub(crate) fn fixture() -> EvmCall {
    let selector=Keccak256::digest(b"swapTokensSingleV3ERC20ToERC20(bytes32,string,string,address,uint256,(address,address,address,address,uint256,bytes,bool))");
    let mut data = selector[..4].to_vec();
    for word in [
        [0; 32],
        evm_uint_word(192),
        evm_uint_word(224),
        evm_address_word(SENDER).unwrap(),
        evm_uint_word(1_990_000),
        evm_uint_word(256),
        evm_uint_word(0),
        evm_uint_word(0),
        evm_address_word("0x4444444444444444444444444444444444444444").unwrap(),
        evm_address_word("0x4444444444444444444444444444444444444444").unwrap(),
        evm_address_word(INPUT).unwrap(),
        evm_address_word(OUTPUT).unwrap(),
        evm_uint_word(1_000_000),
        evm_uint_word(224),
        evm_uint_word(1),
        evm_uint_word(4),
    ] {
        data.extend(word);
    }
    data.extend([1, 2, 3, 4]);
    data.extend([0; 28]);
    EvmCall {
        to: DIAMOND.into(),
        value: 0,
        data,
    }
}
#[test]
fn binds_li_fi_single_swap_to_input_output_recipient_and_minimum() {
    let call = fixture();
    assert!(verify_call(&call, SENDER, INPUT, OUTPUT, 1_000_000, 1_990_000).is_ok());
    for slot in [3, 4, 8, 9, 10, 11, 12, 13, 14] {
        let mut other = fixture();
        let offset = 4 + slot * 32;
        other.data[offset..offset + 32].copy_from_slice(&[0; 32]);
        // requiresDeposit is ignored by the single-swap facet, so both booleans are valid.
        if slot != 14 {
            assert!(
                verify_call(&other, SENDER, INPUT, OUTPUT, 1_000_000, 1_990_000).is_err(),
                "slot {slot}"
            );
        }
    }
    let mut other = fixture();
    other.to = INPUT.into();
    assert!(verify_call(&other, SENDER, INPUT, OUTPUT, 1_000_000, 1_990_000).is_err());
    let mut other = fixture();
    other.value = 1;
    assert!(verify_call(&other, SENDER, INPUT, OUTPUT, 1_000_000, 1_990_000).is_err());
}
#[test]
fn rejects_other_facets_and_malformed_li_fi_abi() {
    for size in [0, 3, 4, 35, 195, 300] {
        let mut call = fixture();
        call.data.truncate(size);
        assert!(verify_call(&call, SENDER, INPUT, OUTPUT, 1_000_000, 1_990_000).is_err());
    }
    let mut call = fixture();
    call.data[..4].copy_from_slice(&[0; 4]);
    assert!(verify_call(&call, SENDER, INPUT, OUTPUT, 1_000_000, 1_990_000).is_err());
    let mut call = fixture();
    call.data[4 + 5 * 32..4 + 6 * 32].fill(255);
    assert!(verify_call(&call, SENDER, INPUT, OUTPUT, 1_000_000, 1_990_000).is_err());
}

#[test]
fn verifies_live_lifi_quote_with_included_fee_collection() {
    let body: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/lifi-evm.json")).unwrap();
    let request = &body["transactionRequest"];
    let call = EvmCall {
        to: request["to"].as_str().unwrap().into(),
        value: u128::from_str_radix(
            request["value"].as_str().unwrap().trim_start_matches("0x"),
            16,
        )
        .unwrap(),
        data: hex::decode(request["data"].as_str().unwrap().trim_start_matches("0x")).unwrap(),
    };
    let sender = body["action"]["fromAddress"].as_str().unwrap();
    let input = body["action"]["fromToken"]["address"].as_str().unwrap();
    let output = body["action"]["toToken"]["address"].as_str().unwrap();
    let amount = body["action"]["fromAmount"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let minimum = body["estimate"]["toAmountMin"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    verify_call(&call, sender, input, output, amount, minimum).unwrap();
    let mut changed = EvmCall {
        to: call.to.clone(),
        value: call.value,
        data: call.data.clone(),
    };
    // The second tuple must never pull an additional deposit from the wallet.
    let data = &mut changed.data[4..];
    let array_offset = number(data, 5).unwrap() as usize;
    let second_offset = number(&data[array_offset..], 2).unwrap() as usize;
    let deposit_word = array_offset + 32 + second_offset + 6 * 32;
    data[deposit_word..deposit_word + 32].copy_from_slice(&crate::tx::evm::evm_uint_word(1));
    assert!(verify_call(&changed, sender, input, output, amount, minimum).is_err());
}
