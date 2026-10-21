use super::*;
use solana_instruction::{AccountMeta, Instruction};
use solana_message::{Message, VersionedMessage};

pub(crate) fn fixture(sender: &str) -> (String, String, String) {
    let input = Pubkey::from([1; 32]).to_string();
    let output = Pubkey::from([2; 32]).to_string();
    let mut data = Sha256::digest(b"global:route")[..8].to_vec();
    data.extend(1u32.to_le_bytes());
    data.extend([0, 100, 0, 1]); // one Saber route-plan step
    data.extend(1_000_000u64.to_le_bytes());
    data.extend(2_000_000u64.to_le_bytes());
    data.extend(50u16.to_le_bytes());
    data.push(0);
    let instruction = Instruction {
        program_id: key(JUPITER).unwrap(),
        accounts: vec![
            AccountMeta::new_readonly(key(TOKEN).unwrap(), false),
            AccountMeta::new_readonly(key(sender).unwrap(), true),
            AccountMeta::new(associated(sender, &input).unwrap(), false),
            AccountMeta::new(associated(sender, &output).unwrap(), false),
            AccountMeta::new(associated(sender, &output).unwrap(), false),
            AccountMeta::new_readonly(key(&output).unwrap(), false),
            AccountMeta::new_readonly(key(JUPITER).unwrap(), false),
            AccountMeta::new_readonly(Pubkey::from([4; 32]), false),
            AccountMeta::new_readonly(key(JUPITER).unwrap(), false),
        ],
        data,
    };
    let mut message = Message::new(&[instruction], Some(&key(sender).unwrap()));
    message.recent_blockhash = solana_hash::Hash::from([7; 32]);
    let tx = VersionedTransaction {
        signatures: vec![Default::default()],
        message: VersionedMessage::Legacy(message),
    };
    (
        STANDARD.encode(wincode::serialize(&tx).unwrap()),
        input,
        output,
    )
}
#[test]
fn binds_native_swap_instruction_to_authority_assets_amount_and_minimum() {
    let sender = Pubkey::from([3; 32]).to_string();
    let (raw, input, output) = fixture(&sender);
    assert!(
        validate(
            &raw,
            &[],
            &sender,
            &input,
            &output,
            1_000_000,
            1_990_000,
            50
        )
        .is_ok()
    );
    assert!(validate(&raw, &[], &sender, &input, &output, 999_999, 1_990_000, 50).is_err());
    assert!(
        validate(
            &raw,
            &[],
            &sender,
            &input,
            &output,
            1_000_000,
            1_990_001,
            50
        )
        .is_err()
    );
    assert!(
        validate(
            &raw,
            &[],
            &sender,
            &input,
            &output,
            1_000_000,
            1_990_000,
            10
        )
        .is_err()
    );
    assert!(
        validate(
            &raw,
            &[],
            &sender,
            &output,
            &input,
            1_000_000,
            1_990_000,
            50
        )
        .is_err()
    );
    assert!(
        validate(
            &raw,
            &[],
            &Pubkey::from([9; 32]).to_string(),
            &input,
            &output,
            1_000_000,
            1_990_000,
            50
        )
        .is_err()
    );
}
#[test]
fn rejects_foreign_programs_extra_signers_and_unsupported_transactions() {
    let sender = Pubkey::from([3; 32]).to_string();
    let (raw, input, output) = fixture(&sender);
    for mutation in 0..3 {
        let mut tx = decode(&raw).unwrap();
        let VersionedMessage::Legacy(message) = &mut tx.message else {
            unreachable!()
        };
        match mutation {
            0 => {
                message.account_keys[usize::from(message.instructions[0].program_id_index)] =
                    Pubkey::from([8; 32])
            }
            1 => message.header.num_required_signatures = 2,
            _ => message.instructions[0].data[0] = 0,
        }
        let altered = STANDARD.encode(wincode::serialize(&tx).unwrap());
        assert!(
            validate(
                &altered,
                &[],
                &sender,
                &input,
                &output,
                1_000_000,
                1_990_000,
                50
            )
            .is_err()
        );
    }
    assert!(decode("invalid").is_err());
    assert!(decode(&STANDARD.encode(vec![0; 1233])).is_err());
}

#[test]
fn validates_live_lifi_solana_route_v2_and_rejects_extra_fee_or_changed_authority() {
    let body: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/lifi-solana.json")).unwrap();
    let raw = body["transactionRequest"]["data"].as_str().unwrap();
    let lookups: Vec<SolanaLookup> = serde_json::from_value(body["lookups"].clone()).unwrap();
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
    validate(raw, &lookups, sender, input, output, amount, minimum, 50).unwrap();
    for mutation in 0..4 {
        let mut tx = decode(raw).unwrap();
        let VersionedMessage::V0(message) = &mut tx.message else {
            unreachable!()
        };
        match mutation {
            0 => message.instructions[3].data[4..12].copy_from_slice(&25001u64.to_le_bytes()),
            1 => message.account_keys[1] = Pubkey::from([99; 32]),
            2 => message.instructions[9].data[8..16].copy_from_slice(&9_975_001u64.to_le_bytes()),
            _ => message.instructions[9].accounts[0] = 1,
        }
        let changed = STANDARD.encode(wincode::serialize(&tx).unwrap());
        assert!(
            validate(
                &changed, &lookups, sender, input, output, amount, minimum, 50
            )
            .is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn validates_live_lifi_spl_input_and_native_output_without_extra_spending() {
    let body: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/lifi-solana-spl.json")).unwrap();
    let raw = body["transactionRequest"]["data"].as_str().unwrap();
    let lookups: Vec<SolanaLookup> = serde_json::from_value(body["lookups"].clone()).unwrap();
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
    validate(raw, &lookups, sender, input, output, amount, minimum, 50).unwrap();
    let mut tx = decode(raw).unwrap();
    let VersionedMessage::V0(message) = &mut tx.message else {
        unreachable!()
    };
    message.instructions[3].data[1..9].copy_from_slice(&2501u64.to_le_bytes());
    let changed = STANDARD.encode(wincode::serialize(&tx).unwrap());
    assert!(
        validate(
            &changed, &lookups, sender, input, output, amount, minimum, 50
        )
        .is_err()
    );
}
