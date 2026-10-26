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
fn token2022_v2_routes_accept_classic_and_native_pair_legs_with_correct_programs() {
    let sender = Pubkey::from([3; 32]).to_string();
    for (classic_input, classic_output, native_input, native_output) in [
        (true, false, false, false),
        (false, true, false, false),
        (true, false, true, false),
        (false, true, false, true),
    ] {
        let (raw, mut tokens) = token2022_fixture(&sender, false);
        for (token, classic, native) in [
            (&mut tokens.input, classic_input, native_input),
            (&mut tokens.output, classic_output, native_output),
        ] {
            if classic {
                token.program = TOKEN.into();
                token.extensions = vec![];
                token.fee_schedule = None;
                token.account_len = 165;
            }
            if native {
                token.mint = WSOL.into();
                token.decimals = 9;
            }
        }
        let tx = decode(&raw).unwrap();
        let keys = tx.message.static_account_keys();
        let readonly_start =
            keys.len() - usize::from(tx.message.header().num_readonly_unsigned_accounts);
        let mut swap = Instruction {
            program_id: key(JUPITER).unwrap(),
            accounts: tx.message.instructions()[1]
                .accounts
                .iter()
                .map(|index| {
                    let index = usize::from(*index);
                    if index < readonly_start {
                        AccountMeta::new(keys[index], index == 0)
                    } else {
                        AccountMeta::new_readonly(keys[index], false)
                    }
                })
                .collect(),
            data: tx.message.instructions()[1].data.clone(),
        };
        swap.accounts[1].pubkey = tokens.input.associated(&sender).unwrap();
        swap.accounts[2].pubkey = tokens.output.associated(&sender).unwrap();
        swap.accounts[3].pubkey = key(&tokens.input.mint).unwrap();
        swap.accounts[4].pubkey = key(&tokens.output.mint).unwrap();
        swap.accounts[5].pubkey = key(&tokens.input.program).unwrap();
        swap.accounts[6].pubkey = key(&tokens.output.program).unwrap();
        let create = |token: &crate::solana_swap_tokens::SwapToken| {
            spl_associated_token_account_interface::instruction::create_associated_token_account_idempotent(&key(&sender).unwrap(),&key(&sender).unwrap(),&key(&token.mint).unwrap(),&key(&token.program).unwrap())
        };
        let mut instructions = vec![create(&tokens.output)];
        if native_input {
            instructions.push(create(&tokens.input));
            instructions.push(solana_system_interface::instruction::transfer(
                &key(&sender).unwrap(),
                &tokens.input.associated(&sender).unwrap(),
                1_000_000,
            ));
            instructions.push(Instruction {
                program_id: key(TOKEN).unwrap(),
                accounts: vec![AccountMeta::new(
                    tokens.input.associated(&sender).unwrap(),
                    false,
                )],
                data: vec![17],
            });
        }
        instructions.push(swap);
        if native_output {
            instructions.push(Instruction {
                program_id: key(TOKEN).unwrap(),
                accounts: vec![
                    AccountMeta::new(tokens.output.associated(&sender).unwrap(), false),
                    AccountMeta::new(key(&sender).unwrap(), false),
                    AccountMeta::new_readonly(key(&sender).unwrap(), true),
                ],
                data: vec![9],
            });
        }
        let message = solana_message::v0::Message::try_compile(
            &key(&sender).unwrap(),
            &instructions,
            &[],
            tx.message.recent_blockhash().clone(),
        )
        .unwrap();
        let tx = VersionedTransaction {
            signatures: vec![Default::default()],
            message: VersionedMessage::V0(message),
        };
        let raw = STANDARD.encode(wincode::serialize(&tx).unwrap());
        validate_with_tokens(
            &raw,
            &[],
            &sender,
            if native_input {
                NATIVE
            } else {
                &tokens.input.mint
            },
            if native_output {
                NATIVE
            } else {
                &tokens.output.mint
            },
            1_000_000,
            1_990_000,
            50,
            Some(&tokens),
        )
        .unwrap();
    }
}

#[test]
fn token2022_v2_routes_bind_both_token_programs_accounts_and_net_minimum() {
    let sender = Pubkey::from([3; 32]).to_string();
    for shared in [false, true] {
        let (raw, tokens) = token2022_fixture(&sender, shared);
        validate_with_tokens(
            &raw,
            &[],
            &sender,
            &tokens.input.mint,
            &tokens.output.mint,
            1_000_000,
            1_990_000,
            50,
            Some(&tokens),
        )
        .unwrap();
        assert!(
            validate(
                &raw,
                &[],
                &sender,
                &tokens.input.mint,
                &tokens.output.mint,
                1_000_000,
                1_990_000,
                50
            )
            .is_err()
        );
        for mutation in 0..5 {
            let mut tx = decode(&raw).unwrap();
            let VersionedMessage::V0(message) = &mut tx.message else {
                unreachable!()
            };
            let ix = &mut message.instructions[1];
            match mutation {
                0 => {
                    let i = ix.accounts[if shared { 8 } else { 5 }] as usize;
                    message.account_keys[i] = key(TOKEN).unwrap();
                }
                1 => {
                    let i = ix.accounts[if shared { 5 } else { 2 }] as usize;
                    message.account_keys[i] = Pubkey::from([9; 32]);
                }
                2 => {
                    let at = if shared { 9 } else { 8 };
                    ix.data[at..at + 8].copy_from_slice(&999_999u64.to_le_bytes());
                }
                3 => {
                    let at = if shared { 27 } else { 26 };
                    ix.data[at] = 1;
                }
                _ => {
                    let i = message.instructions[0].accounts[5] as usize;
                    message.account_keys[i] = key(TOKEN).unwrap();
                }
            }
            let changed = STANDARD.encode(wincode::serialize(&tx).unwrap());
            assert!(
                validate_with_tokens(
                    &changed,
                    &[],
                    &sender,
                    &tokens.input.mint,
                    &tokens.output.mint,
                    1_000_000,
                    1_990_000,
                    50,
                    Some(&tokens)
                )
                .is_err(),
                "{shared} {mutation}"
            );
        }
    }
}

pub(crate) fn token2022_fixture(
    sender: &str,
    shared: bool,
) -> (String, crate::solana_swap_tokens::SwapTokens) {
    use crate::solana_swap_tokens::{SwapToken, SwapTokens};
    let token = |seed| SwapToken {
        mint: Pubkey::from([seed; 32]).to_string(),
        program: crate::solana_tokens::TOKEN_2022.into(),
        decimals: 6,
        extensions: vec![1],
        fee_schedule: Some(crate::dto::SolanaTransferFeeSchedule {
            epoch: "0".into(),
            basis_points: 100,
            maximum_fee: "1000".into(),
        }),
        account_len: 182,
    };
    let tokens = SwapTokens {
        input: token(1),
        output: token(2),
    };
    let mut data = Sha256::digest(if shared {
        &b"global:shared_accounts_route_v2"[..]
    } else {
        &b"global:route_v2"[..]
    })[..8]
        .to_vec();
    if shared {
        data.push(0);
    }
    data.extend(1_000_000u64.to_le_bytes());
    data.extend(2_000_000u64.to_le_bytes());
    data.extend(50u16.to_le_bytes());
    data.extend([0; 4]);
    data.extend(1u32.to_le_bytes());
    data.extend([0, 16, 39, 0, 1]);
    let input = tokens.input.associated(sender).unwrap();
    let output = tokens.output.associated(sender).unwrap();
    let readonly = |address| AccountMeta::new_readonly(address, false);
    let accounts = if shared {
        vec![
            readonly(Pubkey::from([4; 32])),
            AccountMeta::new_readonly(key(sender).unwrap(), true),
            AccountMeta::new(input, false),
            AccountMeta::new(Pubkey::from([5; 32]), false),
            AccountMeta::new(Pubkey::from([6; 32]), false),
            AccountMeta::new(output, false),
            readonly(key(&tokens.input.mint).unwrap()),
            readonly(key(&tokens.output.mint).unwrap()),
            readonly(key(TOKEN_2022).unwrap()),
            readonly(key(TOKEN_2022).unwrap()),
            readonly(Pubkey::from([4; 32])),
            readonly(key(JUPITER).unwrap()),
        ]
    } else {
        vec![
            AccountMeta::new_readonly(key(sender).unwrap(), true),
            AccountMeta::new(input, false),
            AccountMeta::new(output, false),
            readonly(key(&tokens.input.mint).unwrap()),
            readonly(key(&tokens.output.mint).unwrap()),
            readonly(key(TOKEN_2022).unwrap()),
            readonly(key(TOKEN_2022).unwrap()),
            readonly(key(JUPITER).unwrap()),
            readonly(Pubkey::from([4; 32])),
            readonly(key(JUPITER).unwrap()),
        ]
    };
    let create=spl_associated_token_account_interface::instruction::create_associated_token_account_idempotent(&key(sender).unwrap(),&key(sender).unwrap(),&key(&tokens.output.mint).unwrap(),&key(TOKEN_2022).unwrap());
    let swap = Instruction {
        program_id: key(JUPITER).unwrap(),
        accounts,
        data,
    };
    let message = solana_message::v0::Message::try_compile(
        &key(sender).unwrap(),
        &[create, swap],
        &[],
        solana_hash::Hash::from([7; 32]),
    )
    .unwrap();
    let tx = VersionedTransaction {
        signatures: vec![Default::default()],
        message: VersionedMessage::V0(message),
    };
    (STANDARD.encode(wincode::serialize(&tx).unwrap()), tokens)
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
