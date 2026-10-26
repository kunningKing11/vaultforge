use super::*;
use crate::solana_tokens::{
    CLASSIC, TOKEN_2022,
    tests::{extend, holding_bytes, mint_bytes, value},
};
use spl_token_2022_interface::extension::ExtensionType;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
struct MockRpc {
    from: String,
    to: String,
    mint: String,
    program: &'static str,
    mint_data: Mutex<Vec<u8>>,
    holdings: Mutex<Vec<(String, Vec<u8>)>>,
    recipient: Mutex<Option<Vec<u8>>>,
    epoch: AtomicU64,
    balance: AtomicU64,
    fee: AtomicU64,
    rent: AtomicU64,
    bad_simulation: AtomicBool,
    bad_genesis: AtomicBool,
    fail_rpc: AtomicBool,
    change_before_sign: AtomicBool,
    mint_reads: AtomicU64,
    calls: Mutex<Vec<(String, Value)>>,
}
impl MockRpc {
    fn new(program: &'static str, taxed: bool) -> Self {
        let from =
            crate::derivation::derive_addresses_from_mnemonic_filtered(MNEMONIC, "", &["solana"])
                .unwrap()["solana"]
                .clone();
        let to = Pubkey::from([9; 32]).to_string();
        let mint = Pubkey::from([7; 32]).to_string();
        let mut mint_data = mint_bytes(6);
        let mut first = holding_bytes([7; 32], key(&from).unwrap().to_bytes(), 50, false);
        let mut second = holding_bytes([7; 32], key(&from).unwrap().to_bytes(), 100, false);
        if taxed {
            let mut fee = vec![0; 108];
            fee[80..88].copy_from_slice(&10u64.to_le_bytes());
            fee[88..90].copy_from_slice(&100u16.to_le_bytes());
            fee[90..98].copy_from_slice(&5u64.to_le_bytes());
            fee[98..106].copy_from_slice(&20u64.to_le_bytes());
            fee[106..108].copy_from_slice(&200u16.to_le_bytes());
            extend(&mut mint_data, 1, ExtensionType::TransferFeeConfig, &fee);
            extend(
                &mut first,
                2,
                ExtensionType::TransferFeeAmount,
                &999u64.to_le_bytes(),
            );
            extend(
                &mut second,
                2,
                ExtensionType::TransferFeeAmount,
                &888u64.to_le_bytes(),
            );
        }
        let descriptor = inspect_mint(&mint, &value(program, &mint_data)).unwrap();
        let ata = descriptor.associated_address(&from).unwrap();
        Self {
            from,
            to,
            mint,
            program,
            mint_data: Mutex::new(mint_data),
            holdings: Mutex::new(vec![
                (ata, first),
                (Pubkey::from([6; 32]).to_string(), second),
            ]),
            recipient: Mutex::new(None),
            epoch: AtomicU64::new(4),
            balance: AtomicU64::new(1_000_000_000),
            fee: AtomicU64::new(5000),
            rent: AtomicU64::new(2_100_000),
            bad_simulation: AtomicBool::new(false),
            bad_genesis: AtomicBool::new(false),
            fail_rpc: AtomicBool::new(false),
            change_before_sign: AtomicBool::new(false),
            mint_reads: AtomicU64::new(0),
            calls: Mutex::new(vec![]),
        }
    }
}
impl TokenSendRpc for MockRpc {
    async fn token_send_rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push((method.into(), params.clone()));
        if self.fail_rpc.load(Ordering::SeqCst) {
            return Err("RPC unavailable".into());
        }
        Ok(match method {
            "getGenesisHash" => json!(if self.bad_genesis.load(Ordering::SeqCst) {
                "wrong"
            } else {
                "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d"
            }),
            "getAccountInfo" if params[0] == self.mint => {
                if self.mint_reads.fetch_add(1, Ordering::SeqCst) > 0
                    && self.change_before_sign.load(Ordering::SeqCst)
                {
                    self.epoch.store(5, Ordering::SeqCst);
                }
                json!({"value":value(self.program,&self.mint_data.lock().unwrap())})
            }
            "getAccountInfo" => {
                json!({"value":self.recipient.lock().unwrap().as_ref().map(|bytes| value(self.program,bytes))})
            }
            "getEpochInfo" => json!({"epoch":self.epoch.load(Ordering::SeqCst)}),
            "getTokenAccountsByOwner" => {
                assert_eq!(params[1]["programId"], self.program);
                json!({"value":self.holdings.lock().unwrap().iter().map(|(address,bytes)| json!({"pubkey":address,"account":value(self.program,bytes)})).collect::<Vec<_>>()})
            }
            "getMultipleAccounts" => {
                json!({"value":params[0].as_array().unwrap().iter().map(|address| self.holdings.lock().unwrap().iter().find(|(a,_)| Some(a.as_str())==address.as_str()).map(|(_,b)| value(self.program,b))).collect::<Vec<_>>()})
            }
            "getMinimumBalanceForRentExemption" => json!(self.rent.load(Ordering::SeqCst)),
            "getLatestBlockhash" => {
                json!({"value":{"blockhash":Hash::new_from_array([1;32]).to_string()}})
            }
            "getFeeForMessage" => json!({"value":self.fee.load(Ordering::SeqCst)}),
            "getBalance" => json!({"value":self.balance.load(Ordering::SeqCst)}),
            "simulateTransaction" => {
                json!({"value":{"err":if self.bad_simulation.load(Ordering::SeqCst) { json!({"InstructionError":[1,"Custom"]}) } else { Value::Null }}})
            }
            _ => panic!("Unexpected RPC {method}"),
        })
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
}
fn signed(mock: &MockRpc, prepared: PreparedTokenSend) -> SignedTransaction {
    let tx = crate::tx::solana::sign_solana_instructions(crate::tx::solana::SolanaTransferDraft {
        mnemonic: MNEMONIC,
        bip39_passphrase: "",
        from: &mock.from,
        recent_blockhash: &prepared.blockhash,
        fee_lamports: prepared.fee,
        instructions: prepared.instructions,
    })
    .unwrap();
    SignedTransaction {
        solana_token_send: Some(prepared.review),
        token_address: Some(mock.mint.clone()),
        from: mock.from.clone(),
        to: mock.to.clone(),
        symbol: "TOKEN".into(),
        amount: "100".into(),
        note: String::new(),
        destination_tag: None,
        network: "solana".into(),
        nonce: tx.recent_blockhash,
        signed_at: String::new(),
        payload_hash: tx.signature.clone(),
        signature: tx.signature.clone(),
        fee_amount: tx.fee_lamports.to_string(),
        fee_symbol: "SOL".into(),
        total_debit: "100".into(),
        post_balance: "50".into(),
        decimals: 6,
        fiat_value: 0.0,
        raw_tx: Some(tx.raw_tx_base64),
        tx_hash: Some(tx.signature),
    }
}

#[test]
fn charges_each_source_transfer_and_sizes_extended_recipient_rent() {
    let mock = MockRpc::new(TOKEN_2022, true);
    let prepared = runtime()
        .block_on(prepare(&mock, &mock.from, &mock.to, &mock.mint, 100))
        .unwrap();
    assert_eq!(prepared.source_balance, 150);
    assert_eq!(prepared.review.transfer_fee, "2"); // ceil(50*1%) twice, rather than ceil(100*1%)
    assert_eq!(prepared.review.recipient_amount, "98");
    assert_eq!(prepared.review.sources[0].amount, "50");
    assert_eq!(
        prepared.review.fee_schedule.as_ref().unwrap().basis_points,
        100
    );
    assert!(
        mock.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(method, p)| method == "getMinimumBalanceForRentExemption" && p[0] == 182)
    );
    let signed = signed(&mock, prepared);
    let tx = verify_signed(&signed).unwrap();
    for instruction in tx.message.instructions.iter().skip(1) {
        assert_eq!(
            tx.message.account_keys[instruction.program_id_index as usize].to_string(),
            TOKEN_2022
        );
        assert_eq!(&instruction.data[..2], &[26, 1]);
        assert_eq!(
            u64::from_le_bytes(instruction.data[11..19].try_into().unwrap()),
            1
        );
    }
    runtime()
        .block_on(validate_submission(&mock, &signed))
        .unwrap();
}

#[test]
fn preserves_classic_checked_transfers_and_untaxed_token_2022_transfers() {
    for program in [CLASSIC, TOKEN_2022] {
        let mock = MockRpc::new(program, false);
        let prepared = runtime()
            .block_on(prepare(&mock, &mock.from, &mock.to, &mock.mint, 100))
            .unwrap();
        assert_eq!(prepared.review.recipient_amount, "100");
        assert_eq!(prepared.review.transfer_fee, "0");
        let signed = signed(&mock, prepared);
        let tx = verify_signed(&signed).unwrap();
        assert!(
            tx.message
                .instructions
                .iter()
                .skip(1)
                .all(|i| i.data[0] == 12)
        );
        runtime()
            .block_on(validate_submission(&mock, &signed))
            .unwrap();
    }
}

#[test]
fn rejects_changed_fee_schedule_before_signing() {
    let mock = MockRpc::new(TOKEN_2022, true);
    mock.change_before_sign.store(true, Ordering::SeqCst);
    assert!(
        runtime()
            .block_on(prepare(&mock, &mock.from, &mock.to, &mock.mint, 100))
            .unwrap_err()
            .contains("changed")
    );
}

#[test]
fn rejects_frozen_accounts_hooks_and_insufficient_funds() {
    let mock = MockRpc::new(TOKEN_2022, true);
    for (_, bytes) in mock.holdings.lock().unwrap().iter_mut() {
        bytes[108] = 2;
    }
    assert!(
        runtime()
            .block_on(prepare(&mock, &mock.from, &mock.to, &mock.mint, 100))
            .unwrap_err()
            .contains("frozen")
    );
    let mock = MockRpc::new(TOKEN_2022, false);
    extend(
        &mut mock.mint_data.lock().unwrap(),
        1,
        ExtensionType::TransferHook,
        &[0; 64],
    );
    assert!(
        runtime()
            .block_on(prepare(&mock, &mock.from, &mock.to, &mock.mint, 100))
            .unwrap_err()
            .contains("TransferHook")
    );
    let mock = MockRpc::new(TOKEN_2022, true);
    mock.balance.store(5000, Ordering::SeqCst);
    assert!(
        runtime()
            .block_on(prepare(&mock, &mock.from, &mock.to, &mock.mint, 100))
            .unwrap_err()
            .contains("Insufficient SOL")
    );
    let mock = MockRpc::new(TOKEN_2022, true);
    assert!(
        runtime()
            .block_on(prepare(&mock, &mock.from, &mock.to, &mock.mint, 151))
            .unwrap_err()
            .contains("Insufficient live token")
    );
}

#[test]
fn rejects_mutated_review_and_signed_bytes() {
    let mock = MockRpc::new(TOKEN_2022, true);
    let signed = signed(
        &mock,
        runtime()
            .block_on(prepare(&mock, &mock.from, &mock.to, &mock.mint, 100))
            .unwrap(),
    );
    let mut disguised = signed.clone();
    disguised.token_address = None;
    disguised.solana_token_send = None;
    disguised.decimals = 9;
    disguised.total_debit = "5100".into();
    assert!(verify_signed(&disguised).is_err());
    let mut altered = signed.clone();
    altered.to = Pubkey::from([10; 32]).to_string();
    assert!(verify_signed(&altered).is_err());
    let mut altered = signed.clone();
    altered.solana_token_send.as_mut().unwrap().sources[0].fee = "0".into();
    assert!(verify_signed(&altered).is_err());
    let mut altered = signed.clone();
    altered.solana_token_send.as_mut().unwrap().recipient_amount = "100".into();
    assert!(
        runtime()
            .block_on(validate_submission(&mock, &altered))
            .is_err()
    );
    let mut altered = signed.clone();
    altered.amount = "99".into();
    altered.total_debit = "99".into();
    assert!(
        runtime()
            .block_on(validate_submission(&mock, &altered))
            .is_err()
    );
}

#[test]
fn requires_fresh_review_for_fee_rent_balance_and_account_changes() {
    for change in 0..7 {
        let mock = MockRpc::new(TOKEN_2022, true);
        let signed = signed(
            &mock,
            runtime()
                .block_on(prepare(&mock, &mock.from, &mock.to, &mock.mint, 100))
                .unwrap(),
        );
        match change {
            0 => mock.epoch.store(5, Ordering::SeqCst),
            1 => mock.rent.store(2_200_000, Ordering::SeqCst),
            2 => mock.balance.store(1, Ordering::SeqCst),
            3 => mock.holdings.lock().unwrap()[0].1[108] = 2,
            4 => mock.bad_simulation.store(true, Ordering::SeqCst),
            5 => mock.fee.store(6000, Ordering::SeqCst),
            _ => {
                mock.holdings.lock().unwrap()[0].1[64..72].copy_from_slice(&1u64.to_le_bytes());
            }
        }
        assert!(
            runtime()
                .block_on(validate_submission(&mock, &signed))
                .is_err(),
            "change {change}"
        );
    }
}

#[test]
fn validates_existing_recipient_without_charging_rent_and_fails_closed_on_rpc_errors() {
    let mock = MockRpc::new(TOKEN_2022, true);
    let mut recipient = holding_bytes([7; 32], key(&mock.to).unwrap().to_bytes(), 0, false);
    extend(
        &mut recipient,
        2,
        ExtensionType::TransferFeeAmount,
        &0u64.to_le_bytes(),
    );
    *mock.recipient.lock().unwrap() = Some(recipient);
    let prepared = runtime()
        .block_on(prepare(&mock, &mock.from, &mock.to, &mock.mint, 100))
        .unwrap();
    assert_eq!(prepared.review.account_rent_lamports, "0");
    let signed = signed(&mock, prepared);
    runtime()
        .block_on(validate_submission(&mock, &signed))
        .unwrap();
    mock.recipient.lock().unwrap().as_mut().unwrap()[108] = 2;
    assert!(
        runtime()
            .block_on(validate_submission(&mock, &signed))
            .unwrap_err()
            .contains("frozen")
    );
    mock.fail_rpc.store(true, Ordering::SeqCst);
    assert!(
        runtime()
            .block_on(validate_submission(&mock, &signed))
            .unwrap_err()
            .contains("RPC unavailable")
    );
    mock.fail_rpc.store(false, Ordering::SeqCst);
    mock.bad_genesis.store(true, Ordering::SeqCst);
    assert!(
        runtime()
            .block_on(validate_submission(&mock, &signed))
            .unwrap_err()
            .contains("mainnet")
    );
}

#[test]
fn verifies_native_send_identity_to_prevent_token_validation_bypass() {
    let mock = MockRpc::new(CLASSIC, false);
    let mut signed = signed(
        &mock,
        runtime()
            .block_on(prepare(&mock, &mock.from, &mock.to, &mock.mint, 100))
            .unwrap(),
    );
    let native = crate::tx::solana::sign_solana_transfer_with_blockhash(
        MNEMONIC,
        "",
        &mock.from,
        &mock.to,
        100,
        &signed.nonce,
        5000,
    )
    .unwrap();
    signed.solana_token_send = None;
    signed.token_address = None;
    signed.decimals = 9;
    signed.total_debit = "5100".into();
    signed.signature = native.signature.clone();
    signed.payload_hash = native.signature.clone();
    signed.tx_hash = Some(native.signature);
    signed.raw_tx = Some(native.raw_tx_base64);
    verify_signed(&signed).unwrap();
    signed.amount = "101".into();
    signed.total_debit = "5101".into();
    assert!(verify_signed(&signed).is_err());
}
