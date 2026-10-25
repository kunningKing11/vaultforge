use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use solana_message::VersionedMessage;
use solana_signer::Signer;

fn fixture() -> (solana_keypair::Keypair, JupiterOrder, OrderRequest) {
    let signer = solana_keypair::keypair_from_seed(&[3; 32]).unwrap();
    let (raw, input, output) = encoding::tests::fixture(&signer.pubkey().to_string());
    let mut tx = encoding::decode(&raw).unwrap();
    let VersionedMessage::Legacy(message) = tx.message else {
        unreachable!()
    };
    tx.message = VersionedMessage::V0(solana_message::v0::Message {
        header: message.header,
        account_keys: message.account_keys,
        recent_blockhash: message.recent_blockhash,
        instructions: message.instructions,
        address_table_lookups: vec![],
    });
    let order = JupiterOrder {
        request_id: "jupiter-request".into(),
        router: "metis".into(),
        transaction_base64: STANDARD.encode(wincode::serialize(&tx).unwrap()),
        quoted_output_base_units: "2000000".into(),
        minimum_output_base_units: "1990000".into(),
        raw_response: serde_json::json!({"feeBps":0,"gasless":false,"lastValidBlockHeight":"100"}),
    };
    let request = OrderRequest {
        taker: signer.pubkey().to_string(),
        input_mint: input,
        output_mint: output,
        amount_base_units: "1000000".into(),
        slippage_bps: 50,
    };
    (signer, order, request)
}

#[test]
fn accepts_declared_input_and_output_fees_without_changing_the_signed_message() {
    for from_input in [false, true] {
        for embedded in [false, true] {
            let (signer, mut order, request) = fixture();
            add_fee(&mut order, &request, from_input, embedded);
            let original = encoding::decode(&order.transaction_base64).unwrap();
            let validated = validate_order(&order, &request, &[]).unwrap();
            assert_eq!(validated.transaction.message, original.message);
            assert_eq!(validated.fee_token_accounts.len(), 1);
            let signed =
                VersionedTransaction::try_new(validated.transaction.message, &[&signer]).unwrap();
            verify_signed_order(
                &order,
                &STANDARD.encode(wincode::serialize(&signed).unwrap()),
                &request.taker,
            )
            .unwrap();
            // Existing Relay/LI.FI validation must not inherit direct Jupiter fees.
            assert!(
                encoding::validate(
                    &order.transaction_base64,
                    &[],
                    &request.taker,
                    &request.input_mint,
                    &request.output_mint,
                    1_000_000,
                    order.minimum_output_base_units.parse().unwrap(),
                    50
                )
                .is_err()
            );
        }
    }
}

#[test]
fn accepts_native_fees_without_spending_more_than_the_selected_sol_amount() {
    use solana_instruction::{AccountMeta, Instruction};
    for from_input in [false, true] {
        for native_transfer in [false, true] {
            let (_, mut order, mut request) = fixture();
            if from_input {
                request.input_mint = encoding::NATIVE.into();
            } else {
                request.output_mint = encoding::NATIVE.into();
            }
            let tx = encoding::decode(&order.transaction_base64).unwrap();
            let mut swap = instructions(&tx, &request.taker).remove(0);
            swap.accounts[2].pubkey =
                encoding::associated(&request.taker, &request.input_mint).unwrap();
            swap.accounts[3].pubkey =
                encoding::associated(&request.taker, &request.output_mint).unwrap();
            swap.accounts[4].pubkey = swap.accounts[3].pubkey;
            swap.accounts[5].pubkey = key(encoding::mint(&request.output_mint)).unwrap();
            save_instructions(&mut order, &request, &[swap]);
            add_fee(&mut order, &request, from_input, false);
            let tx = encoding::decode(&order.transaction_base64).unwrap();
            let mut list = instructions(&tx, &request.taker);
            let sender = key(&request.taker).unwrap();
            let fee: u64 = order.raw_response["platformFee"]["amount"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
            let fee_index = if from_input { 0 } else { 1 };
            if native_transfer {
                let mut data = 2u32.to_le_bytes().to_vec();
                data.extend(fee.to_le_bytes());
                list[fee_index] = Instruction {
                    program_id: key(encoding::NATIVE).unwrap(),
                    accounts: vec![
                        AccountMeta::new(sender, true),
                        AccountMeta::new(Pubkey::from([42; 32]), false),
                    ],
                    data,
                };
            }
            let wsol = encoding::associated(&request.taker, encoding::NATIVE).unwrap();
            if from_input {
                let wrapped = 1_000_000u64 - if native_transfer { fee } else { 0 };
                let mut data = 2u32.to_le_bytes().to_vec();
                data.extend(wrapped.to_le_bytes());
                list.insert(
                    0,
                    Instruction {
                        program_id: key(encoding::NATIVE).unwrap(),
                        accounts: vec![
                            AccountMeta::new(sender, true),
                            AccountMeta::new(wsol, false),
                        ],
                        data,
                    },
                );
                list.insert(
                    1,
                    Instruction {
                        program_id: key("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap(),
                        accounts: vec![AccountMeta::new(wsol, false)],
                        data: vec![17],
                    },
                );
            } else {
                let close = Instruction {
                    program_id: key("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap(),
                    accounts: vec![
                        AccountMeta::new(wsol, false),
                        AccountMeta::new(sender, false),
                        AccountMeta::new_readonly(sender, true),
                    ],
                    data: vec![9],
                };
                if native_transfer {
                    list.insert(1, close);
                } else {
                    list.push(close);
                }
            }
            save_instructions(&mut order, &request, &list);
            validate_order(&order, &request, &[]).unwrap();
            let wsol = encoding::associated(&request.taker, encoding::NATIVE).unwrap();
            let mut changed = list.clone();
            if from_input {
                let wrap = changed
                    .iter_mut()
                    .find(|ix| {
                        ix.program_id == key(encoding::NATIVE).unwrap()
                            && ix.accounts.get(1).is_some_and(|meta| meta.pubkey == wsol)
                    })
                    .unwrap();
                wrap.accounts[0].pubkey = Pubkey::from([43; 32]);
                save_instructions(&mut order, &request, &changed);
                assert!(validate_order(&order, &request, &[]).is_err());

                let mut changed = list.clone();
                let wrap = changed
                    .iter_mut()
                    .find(|ix| {
                        ix.program_id == key(encoding::NATIVE).unwrap()
                            && ix.accounts.get(1).is_some_and(|meta| meta.pubkey == wsol)
                    })
                    .unwrap();
                wrap.accounts[1].pubkey = Pubkey::from([45; 32]);
                save_instructions(&mut order, &request, &changed);
                assert!(validate_order(&order, &request, &[]).is_err());

                let mut changed = list.clone();
                let sync = changed
                    .iter_mut()
                    .find(|ix| {
                        ix.program_id == key("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap()
                            && ix.data == [17]
                    })
                    .unwrap();
                sync.accounts[0].pubkey = Pubkey::from([44; 32]);
                save_instructions(&mut order, &request, &changed);
                assert!(validate_order(&order, &request, &[]).is_err());
            } else {
                for account_index in [1, 2] {
                    let mut changed = list.clone();
                    let close = changed
                        .iter_mut()
                        .find(|ix| {
                            ix.program_id
                                == key("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap()
                                && ix.data == [9]
                        })
                        .unwrap();
                    close.accounts[account_index].pubkey = Pubkey::from([44; 32]);
                    save_instructions(&mut order, &request, &changed);
                    assert!(validate_order(&order, &request, &[]).is_err());
                }
            }
            if from_input {
                list[0].data[4..12].copy_from_slice(&1_000_001u64.to_le_bytes());
                save_instructions(&mut order, &request, &list);
                assert!(validate_order(&order, &request, &[]).is_err());
            }
        }
    }
}

#[test]
fn accepts_shared_account_platform_fees_and_route_v2_explicit_fees() {
    use solana_instruction::AccountMeta;
    for from_input in [false, true] {
        let (_, mut order, request) = fixture();
        add_fee(&mut order, &request, from_input, true);
        let tx = encoding::decode(&order.transaction_base64).unwrap();
        let mut swap = instructions(&tx, &request.taker).remove(0);
        let old = swap.accounts.clone();
        let extra = AccountMeta::new(Pubkey::from([90; 32]), false);
        swap.accounts = vec![
            old[0].clone(),
            extra.clone(),
            old[1].clone(),
            old[2].clone(),
            extra.clone(),
            extra.clone(),
            old[3].clone(),
            AccountMeta::new(key(&request.input_mint).unwrap(), false),
            old[5].clone(),
            old[6].clone(),
            old[8].clone(),
            old[7].clone(),
            old[8].clone(),
        ];
        let mut data = Sha256::digest(b"global:shared_accounts_route")[..8].to_vec();
        data.push(0);
        data.extend(&swap.data[8..]);
        swap.data = data;
        save_instructions(&mut order, &request, &[swap]);
        validate_order(&order, &request, &[]).unwrap();

        let (_, mut order, request) = fixture();
        add_fee(&mut order, &request, from_input, false);
        let tx = encoding::decode(&order.transaction_base64).unwrap();
        let mut list = instructions(&tx, &request.taker);
        let swap = list
            .iter_mut()
            .find(|ix| ix.program_id == key(encoding::JUPITER).unwrap())
            .unwrap();
        let old = swap.accounts.clone();
        let tail = swap.data.len() - 19;
        let mut data = Sha256::digest(b"global:route_v2")[..8].to_vec();
        data.extend(&swap.data[tail..tail + 18]);
        data.extend([0; 4]);
        data.extend(1u32.to_le_bytes());
        data.extend([0, 0x10, 0x27, 0, 1]);
        swap.data = data;
        swap.accounts = vec![
            old[1].clone(),
            old[2].clone(),
            old[3].clone(),
            AccountMeta::new(key(&request.input_mint).unwrap(), false),
            old[5].clone(),
            old[0].clone(),
            old[0].clone(),
            old[4].clone(),
            old[7].clone(),
            old[8].clone(),
        ];
        save_instructions(&mut order, &request, &list);
        validate_order(&order, &request, &[]).unwrap();
    }
}

pub(crate) fn add_fee(
    order: &mut JupiterOrder,
    request: &OrderRequest,
    from_input: bool,
    embedded: bool,
) {
    use solana_instruction::{AccountMeta, Instruction};
    let original = encoding::decode(&order.transaction_base64).unwrap();
    let keys = original.message.static_account_keys();
    let swap = original
        .message
        .instructions()
        .iter()
        .find(|ix| keys[usize::from(ix.program_id_index)] == key(encoding::JUPITER).unwrap())
        .unwrap();
    let sender = key(&request.taker).unwrap();
    let collector = Pubkey::from([42; 32]);
    let mut swap = Instruction {
        program_id: key(encoding::JUPITER).unwrap(),
        accounts: swap
            .accounts
            .iter()
            .map(|index| {
                let account = keys[usize::from(*index)];
                AccountMeta::new(account, account == sender)
            })
            .collect(),
        data: swap.data.clone(),
    };
    let input: u64 = request.amount_base_units.parse().unwrap();
    let fee_amount = if from_input {
        input * 20 / 10_000
    } else {
        4_000
    };
    let mint = if from_input {
        encoding::mint(&request.input_mint)
    } else {
        encoding::mint(&request.output_mint)
    };
    let tail = swap.data.len() - 19;
    if embedded {
        swap.accounts[6] = AccountMeta::new(collector, false);
        *swap.data.last_mut().unwrap() = 20;
    } else if from_input {
        swap.data[tail..tail + 8].copy_from_slice(&(input - fee_amount).to_le_bytes());
    } else {
        swap.data[tail + 16..tail + 18].copy_from_slice(&0u16.to_le_bytes());
    }
    if !from_input {
        order.quoted_output_base_units = (2_000_000 - fee_amount).to_string();
        order.minimum_output_base_units = crate::providers::uniswap::minimum_output(
            2_000_000 - u128::from(fee_amount),
            request.slippage_bps,
        )
        .unwrap()
        .to_string();
    }
    let mut instructions = vec![swap];
    if !embedded {
        let mut data = vec![3];
        data.extend(fee_amount.to_le_bytes());
        let transfer = Instruction {
            program_id: key("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap(),
            accounts: vec![
                AccountMeta::new(encoding::associated(&request.taker, mint).unwrap(), false),
                AccountMeta::new(collector, false),
                AccountMeta::new_readonly(sender, true),
            ],
            data,
        };
        if from_input {
            instructions.insert(0, transfer);
        } else {
            instructions.push(transfer);
        }
    }
    let message = solana_message::v0::Message::try_compile(
        &sender,
        &instructions,
        &[],
        original.message.recent_blockhash().to_owned(),
    )
    .unwrap();
    let tx = VersionedTransaction {
        signatures: vec![Default::default()],
        message: VersionedMessage::V0(message),
    };
    order.transaction_base64 = STANDARD.encode(wincode::serialize(&tx).unwrap());
    order.raw_response["feeBps"] = 20.into();
    order.raw_response["feeMint"] = mint.into();
    order.raw_response["platformFee"] =
        serde_json::json!({"amount":fee_amount.to_string(),"feeBps":20,"feeMint":mint});
}

#[test]
fn binds_original_message_and_signer_before_execution() {
    let (signer, order, request) = fixture();
    let validated = validate_order(&order, &request, &[]).unwrap();
    assert_eq!(validated.wallet_signer_index, 0);
    let signed = VersionedTransaction::try_new(validated.transaction.message, &[&signer]).unwrap();
    let raw = STANDARD.encode(wincode::serialize(&signed).unwrap());
    verify_signed_order(&order, &raw, &request.taker).unwrap();
    let mut changed = signed.clone();
    let VersionedMessage::V0(message) = &mut changed.message else {
        unreachable!()
    };
    message.recent_blockhash = solana_hash::Hash::from([8; 32]);
    let changed = VersionedTransaction::try_new(changed.message, &[&signer]).unwrap();
    assert!(
        verify_signed_order(
            &order,
            &STANDARD.encode(wincode::serialize(&changed).unwrap()),
            &request.taker
        )
        .is_err()
    );
    assert!(verify_signed_order(&order, &order.transaction_base64, &request.taker).is_err());
}

#[test]
fn rejects_changed_assets_recipient_and_slippage() {
    for mutation in 0..5 {
        let (_, order, mut request) = fixture();
        match mutation {
            0 => request.taker = encoding::JUPITER.into(),
            1 => request.input_mint = encoding::WSOL.into(),
            2 => request.output_mint = encoding::WSOL.into(),
            3 => request.slippage_bps = 1,
            _ => request.amount_base_units = "1000001".into(),
        }
        assert!(
            validate_order(&order, &request, &[]).is_err(),
            "mutation {mutation}"
        );
    }
}

fn instructions(tx: &VersionedTransaction, sender: &str) -> Vec<solana_instruction::Instruction> {
    use solana_instruction::AccountMeta;
    let keys = tx.message.static_account_keys();
    tx.message
        .instructions()
        .iter()
        .map(|ix| solana_instruction::Instruction {
            program_id: keys[usize::from(ix.program_id_index)],
            accounts: ix
                .accounts
                .iter()
                .map(|i| {
                    let account = keys[usize::from(*i)];
                    AccountMeta::new(account, account == key(sender).unwrap())
                })
                .collect(),
            data: ix.data.clone(),
        })
        .collect()
}

fn save_instructions(
    order: &mut JupiterOrder,
    request: &OrderRequest,
    instructions: &[solana_instruction::Instruction],
) {
    let original = encoding::decode(&order.transaction_base64).unwrap();
    let message = solana_message::v0::Message::try_compile(
        &key(&request.taker).unwrap(),
        instructions,
        &[],
        original.message.recent_blockhash().to_owned(),
    )
    .unwrap();
    order.transaction_base64 = STANDARD.encode(
        wincode::serialize(&VersionedTransaction {
            signatures: vec![Default::default()],
            message: VersionedMessage::V0(message),
        })
        .unwrap(),
    );
}

#[test]
fn rejects_embedded_fee_mismatches_and_mixed_collection() {
    for mutation in 0..4 {
        let (_, mut order, request) = fixture();
        add_fee(&mut order, &request, false, true);
        let tx = encoding::decode(&order.transaction_base64).unwrap();
        let mut list = instructions(&tx, &request.taker);
        match mutation {
            0 => *list[0].data.last_mut().unwrap() = 21,
            1 => *list[0].data.last_mut().unwrap() = 0,
            2 => order.raw_response["feeBps"] = 0.into(),
            _ => {
                let (_, mut external, external_request) = fixture();
                add_fee(&mut external, &external_request, false, false);
                let tx = encoding::decode(&external.transaction_base64).unwrap();
                let extra = instructions(&tx, &request.taker).pop().unwrap();
                list.push(extra);
            }
        }
        save_instructions(&mut order, &request, &list);
        assert!(
            validate_order(&order, &request, &[]).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn rejects_hidden_duplicate_or_changed_fee_transfers_and_lowered_net_minimum() {
    for mutation in 0..8 {
        let (_, mut order, request) = fixture();
        add_fee(&mut order, &request, false, false);
        let mut tx = encoding::decode(&order.transaction_base64).unwrap();
        let VersionedMessage::V0(message) = &mut tx.message else {
            unreachable!()
        };
        match mutation {
            0 => order.raw_response["platformFee"]["amount"] = "4001".into(),
            1 => order.raw_response["feeMint"] = encoding::WSOL.into(),
            2 => order.raw_response["platformFee"]["feeBps"] = 19.into(),
            3 => message.instructions[1].data[1..9].copy_from_slice(&4_001u64.to_le_bytes()),
            4 => message.instructions.push(message.instructions[1].clone()),
            5 => message.instructions.swap(0, 1),
            6 => {
                let tail = message.instructions[0].data.len() - 19;
                message.instructions[0].data[tail + 16..tail + 18]
                    .copy_from_slice(&50u16.to_le_bytes());
            }
            _ => order.raw_response["feeBps"] = 0.into(),
        }
        order.transaction_base64 = STANDARD.encode(wincode::serialize(&tx).unwrap());
        assert!(
            validate_order(&order, &request, &[]).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn rejects_unreviewed_engines_fees_and_extra_signers() {
    for mutation in 0..6 {
        let (_, mut order, request) = fixture();
        match mutation {
            0 => order.router = "jupiterz".into(),
            1 => order.raw_response["gasless"] = true.into(),
            2 => order.raw_response["feeBps"] = 2.into(),
            3 => order.raw_response["platformFee"] = serde_json::json!({"amount":"1"}),
            4 => {
                let mut tx = encoding::decode(&order.transaction_base64).unwrap();
                let VersionedMessage::V0(message) = &mut tx.message else {
                    unreachable!()
                };
                message.header.num_required_signatures = 2;
                tx.signatures.push(Default::default());
                order.transaction_base64 = STANDARD.encode(wincode::serialize(&tx).unwrap());
            }
            _ => {
                let mut tx = encoding::decode(&order.transaction_base64).unwrap();
                let VersionedMessage::V0(message) = &mut tx.message else {
                    unreachable!()
                };
                message.instructions[0].data[0] ^= 1;
                order.transaction_base64 = STANDARD.encode(wincode::serialize(&tx).unwrap());
            }
        }
        assert!(
            validate_order(&order, &request, &[]).is_err(),
            "mutation {mutation}"
        );
    }
}
