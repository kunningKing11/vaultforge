use super::*;
use crate::cctp_execution::tests::{execution_state, temp_dir};
use crate::dto::Asset;
use crate::swap_routes::RouteProvider;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

struct MockRpc {
    submitted: AtomicBool,
    finalized: AtomicBool,
    fail_send: AtomicBool,
    bad_genesis: AtomicBool,
    simulation_error: AtomicBool,
    final_height: AtomicU64,
    blockhash_seed: AtomicU64,
    status_slot: AtomicU64,
    appear_at_expiry: AtomicBool,
    sent: Mutex<Vec<String>>,
    before_send: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl Default for MockRpc {
    fn default() -> Self {
        Self {
            submitted: AtomicBool::new(false),
            finalized: AtomicBool::new(false),
            fail_send: AtomicBool::new(false),
            bad_genesis: AtomicBool::new(false),
            simulation_error: AtomicBool::new(false),
            final_height: AtomicU64::new(50),
            blockhash_seed: AtomicU64::new(7),
            status_slot: AtomicU64::new(42),
            appear_at_expiry: AtomicBool::new(false),
            sent: Mutex::new(vec![]),
            before_send: Mutex::new(None),
        }
    }
}
impl SolanaRpc for MockRpc {
    async fn swap_rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        Ok(match method {
            "getGenesisHash" => json!(if self.bad_genesis.load(Ordering::SeqCst) {
                "wrong"
            } else {
                "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d"
            }),
            "getLatestBlockhash" => {
                json!({"context":{"slot":42},"value":{"blockhash":solana_hash::Hash::from([self.blockhash_seed.load(Ordering::SeqCst) as u8;32]).to_string(),"lastValidBlockHeight":100}})
            }
            "getBlockHeight" => {
                if self.appear_at_expiry.load(Ordering::SeqCst) {
                    self.submitted.store(true, Ordering::SeqCst);
                }
                json!(self.final_height.load(Ordering::SeqCst))
            }
            "getSlot" => json!(42),
            "getFeeForMessage" => json!({"value":5000}),
            "getBalance" => json!({"value":1_000_000_000u64}),
            "getTokenAccountsByOwner" => json!({"value":[]}),
            "getAccountInfo" => json!({"value":null}),
            "getMinimumBalanceForRentExemption" => json!(2_039_280),
            "simulateTransaction" => {
                json!({"value":{"err":if self.simulation_error.load(Ordering::SeqCst){json!({"InstructionError":[0,"failure"]})}else{Value::Null},"unitsConsumed":200000}})
            }
            "getSignatureStatuses" => {
                json!({"context":{"slot":self.status_slot.load(Ordering::SeqCst)},"value":[if self.submitted.load(Ordering::SeqCst){json!({"slot":42,"err":null,"confirmationStatus":if self.finalized.load(Ordering::SeqCst){"finalized"}else{"confirmed"}})}else{Value::Null}]})
            }
            "sendTransaction" => {
                let raw = params[0].as_str().unwrap();
                if let Some(hook) = self.before_send.lock().unwrap().take() {
                    hook();
                }
                let tx = encoding::decode(raw).unwrap();
                self.sent.lock().unwrap().push(raw.into());
                self.submitted.store(true, Ordering::SeqCst);
                if self.fail_send.load(Ordering::SeqCst) {
                    return Err("Response lost".into());
                }
                json!(tx.signatures[0].to_string())
            }
            _ => panic!("Unexpected RPC method {method}"),
        })
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn setup() -> (std::path::PathBuf, Arc<Mutex<AppState>>, String) {
    let directory = temp_dir("native-api-swap");
    let state = Arc::new(execution_state(&directory));
    let mut lock = state.lock().unwrap();
    let wallet = lock.wallet.as_mut().unwrap();
    let secret = crate::derivation::solana_secret_key_from_mnemonic(
        &wallet.mnemonic,
        &wallet.bip39_passphrase,
    )
    .unwrap();
    let signer = solana_keypair::keypair_from_seed(&secret)
        .unwrap()
        .pubkey()
        .to_string();
    wallet.addresses.insert("solana".into(), signer.clone());
    wallet.enabled_networks = vec!["solana".into()];
    let (transaction, input, output) = encoding::tests::fixture(&signer);
    wallet.assets = [
        (None, "SOL", 9),
        (Some(input.clone()), "IN", 6),
        (Some(output.clone()), "OUT", 6),
    ]
    .into_iter()
    .map(|(token, symbol, decimals)| Asset {
        network: "solana".into(),
        token_address: token,
        symbol: symbol.into(),
        name: symbol.into(),
        unicode_symbol: None,
        decimals,
        balance: "1000000".into(),
        price_usd: 1.,
        change_24h: 0.,
    })
    .collect();
    let api = ApiSwap {
        provider: RouteProvider::Lifi,
        request_id: "native-request".into(),
        route: vec![],
        transaction: RouteTransaction::Solana {
            transaction,
            lookups: vec![],
        },
    };
    lock.uniswap_quotes.insert(
        "native-quote".into(),
        crate::uniswap::UniswapQuote {
            api: Some(api),
            destination_network: "solana".into(),
            relay: None,
            route: vec![],
            id: "native-quote".into(),
            network: "solana".into(),
            token_in: input,
            token_out: output,
            sender: signer,
            amount_base_units: "1000000".into(),
            quoted_output: "2000000".into(),
            minimum_received: "1990000".into(),
            slippage_bps: 50,
            pool_fee: None,
            deadline: crate::uniswap::now() + 1200,
            approval_required: false,
            approval_max_fee: None,
            swap_max_fee: Some("5000".into()),
            input_symbol: "IN".into(),
            output_symbol: "OUT".into(),
            input_decimals: 6,
            output_decimals: 6,
        },
    );
    drop(lock);
    let id = crate::uniswap::create_swap_operation(&state, "native-quote")
        .unwrap()
        .operation_id;
    (directory, state, id)
}
fn operation(state: &Mutex<AppState>, id: &str) -> Operation {
    state
        .lock()
        .unwrap()
        .wallet
        .as_ref()
        .unwrap()
        .operations
        .iter()
        .find(|o| o.id == id)
        .unwrap()
        .clone()
}

#[test]
fn expires_only_after_history_and_finalized_height_prove_non_inclusion() {
    let (directory, state, id) = setup();
    let provider = MockRpc::default();
    let prepared = runtime().block_on(prepare(&state, &provider, &id)).unwrap();
    assert!(!crate::uniswap::terminal(&operation(&state, &id)));
    provider.final_height.store(101, Ordering::SeqCst);
    runtime().block_on(sync(&state, &provider, &id)).unwrap();
    assert!(expired(&operation(&state, &id)));
    assert!(
        runtime()
            .block_on(submit(&state, &provider, &id, &prepared.tx_hash))
            .is_err()
    );
    assert!(provider.sent.lock().unwrap().is_empty());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn includes_missing_token_account_rent_in_reviewed_and_persisted_budget() {
    use solana_instruction::{AccountMeta, Instruction};
    let (directory, state, id) = setup();
    let original = operation(&state, &id);
    let api = original.swap.as_ref().unwrap().api.as_ref().unwrap();
    let RouteTransaction::Solana { transaction, .. } = &api.transaction else {
        unreachable!()
    };
    let tx = encoding::decode(transaction).unwrap();
    let keys = tx.message.static_account_keys();
    let jupiter = &tx.message.instructions()[0];
    let sender = key(&original.sender).unwrap();
    let output = original.destination_asset.token_address.as_deref().unwrap();
    let create_ata = Instruction {
        program_id: key("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL").unwrap(),
        accounts: vec![
            AccountMeta::new(sender, true),
            AccountMeta::new(
                encoding::associated(&original.sender, output).unwrap(),
                false,
            ),
            AccountMeta::new_readonly(sender, false),
            AccountMeta::new_readonly(key(output).unwrap(), false),
            AccountMeta::new_readonly(key(encoding::NATIVE).unwrap(), false),
            AccountMeta::new_readonly(
                key("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap(),
                false,
            ),
        ],
        data: vec![1],
    };
    let swap = Instruction {
        program_id: keys[usize::from(jupiter.program_id_index)],
        accounts: jupiter
            .accounts
            .iter()
            .map(|i| {
                let account = keys[usize::from(*i)];
                AccountMeta::new(account, account == sender)
            })
            .collect(),
        data: jupiter.data.clone(),
    };
    let message = solana_message::Message::new(&[create_ata, swap], Some(&sender));
    let unsigned = VersionedTransaction {
        signatures: vec![Default::default()],
        message: VersionedMessage::Legacy(message),
    };
    {
        let mut guard = state.lock().unwrap();
        let saved = guard
            .wallet
            .as_mut()
            .unwrap()
            .operations
            .iter_mut()
            .find(|o| o.id == id)
            .unwrap();
        let RouteTransaction::Solana { transaction, .. } = &mut saved
            .swap
            .as_mut()
            .unwrap()
            .api
            .as_mut()
            .unwrap()
            .transaction
        else {
            unreachable!()
        };
        *transaction = STANDARD.encode(wincode::serialize(&unsigned).unwrap());
    }
    let rpc = MockRpc::default();
    let prepared = runtime().block_on(prepare(&state, &rpc, &id)).unwrap();
    assert_eq!(prepared.max_native_fee, "2044280");
    let saved = operation(&state, &id);
    let context = saved.attempts[0].solana_context.as_ref().unwrap();
    assert_eq!(context.fee_lamports, 5000);
    assert_eq!(context.rent_lamports, 2_039_280);
    assert!(rpc.sent.lock().unwrap().is_empty());
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn native_inclusion_allows_another_swap_but_never_reuses_an_existing_signature() {
    let (directory, state, id) = setup();
    let provider = MockRpc::default();
    let first = runtime().block_on(prepare(&state, &provider, &id)).unwrap();
    runtime()
        .block_on(submit(&state, &provider, &id, &first.tx_hash))
        .unwrap();
    runtime().block_on(sync(&state, &provider, &id)).unwrap();
    assert!(crate::uniswap::local_swap_confirmed(&operation(
        &state, &id
    )));
    {
        let mut guard = state.lock().unwrap();
        let mut quote = guard.uniswap_quotes.get("native-quote").unwrap().clone();
        quote.id = "next-native-quote".into();
        guard.uniswap_quotes.insert(quote.id.clone(), quote);
    }
    let next = crate::uniswap::create_swap_operation(&state, "next-native-quote")
        .unwrap()
        .operation_id;
    assert!(
        runtime()
            .block_on(prepare(&state, &provider, &next))
            .is_err()
    );
    assert!(operation(&state, &next).attempts.is_empty());
    provider.blockhash_seed.store(8, Ordering::SeqCst);
    let second = runtime()
        .block_on(prepare(&state, &provider, &next))
        .unwrap();
    assert_ne!(first.tx_hash, second.tx_hash);
    assert_eq!(provider.sent.lock().unwrap().len(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn persists_native_bytes_before_broadcast_and_completes_on_inclusion_while_tracking_finality() {
    let (directory, state, id) = setup();
    let provider = MockRpc::default();
    let password = "native-swap-test";
    let (key, salt) = crate::storage::derive_storage_key(password, Some(&[2; 16])).unwrap();
    {
        let mut lock = state.lock().unwrap();
        lock.encryption_key = Some(key);
        lock.storage_salt = Some(salt);
    }
    let prepared = runtime().block_on(prepare(&state, &provider, &id)).unwrap();
    let saved = operation(&state, &id);
    assert!(saved.attempts[0].evm_context.is_none());
    assert!(saved.swap.as_ref().unwrap().chain_id.is_none());
    let stored = crate::storage::read_stored_wallet(&directory.join("wallet.json"))
        .unwrap()
        .unwrap();
    assert_eq!(stored.version, 15);
    let decrypted = crate::storage::decrypt_wallet(&stored, password).unwrap();
    assert_eq!(decrypted.wallet().operations[0], saved);
    let before = state.clone();
    *provider.before_send.lock().unwrap() = Some(Box::new(move || {
        let lock = before.lock().unwrap();
        let attempt = &lock.wallet.as_ref().unwrap().operations[0].attempts[0];
        assert_eq!(attempt.status, TransactionStatus::SubmissionUnknown);
        let stored = crate::storage::read_stored_wallet(&lock.storage_path)
            .unwrap()
            .unwrap();
        let decrypted = crate::storage::decrypt_wallet(&stored, password).unwrap();
        assert_eq!(
            decrypted.wallet().operations[0].attempts[0].status,
            TransactionStatus::SubmissionUnknown
        );
    }));
    runtime()
        .block_on(submit(&state, &provider, &id, &prepared.tx_hash))
        .unwrap();
    runtime().block_on(sync(&state, &provider, &id)).unwrap();
    assert!(crate::uniswap::terminal(&operation(&state, &id)));
    let view = crate::uniswap::get_state(&state)
        .unwrap()
        .operations
        .remove(0);
    assert_eq!(view.stage, crate::uniswap::SwapStage::Done);
    assert!(view.needs_sync);
    assert_eq!(operation(&state, &id).step, OperationStep::Send);
    assert_eq!(
        crate::uniswap::get_state(&state).unwrap().operations[0]
            .progress
            .title,
        "Swap complete"
    );
    provider.finalized.store(true, Ordering::SeqCst);
    runtime().block_on(sync(&state, &provider, &id)).unwrap();
    assert_eq!(operation(&state, &id).step, OperationStep::Completed);
    assert_eq!(provider.sent.lock().unwrap().len(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn rejects_wrong_genesis_simulation_failure_and_stale_snapshots_without_signing() {
    for failure in 0..3 {
        let (directory, state, id) = setup();
        let provider = MockRpc::default();
        match failure {
            0 => provider.bad_genesis.store(true, Ordering::SeqCst),
            1 => provider.simulation_error.store(true, Ordering::SeqCst),
            _ => state
                .lock()
                .unwrap()
                .wallet
                .as_mut()
                .unwrap()
                .addresses
                .insert(
                    "solana".into(),
                    encoding::key(encoding::JUPITER).unwrap().to_string(),
                )
                .map(|_| ())
                .unwrap(),
        }
        assert!(runtime().block_on(prepare(&state, &provider, &id)).is_err());
        assert!(operation(&state, &id).attempts.is_empty());
        assert!(provider.sent.lock().unwrap().is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
#[test]
fn stale_or_late_signature_observations_cannot_release_native_protection() {
    for late in [false, true] {
        let (directory, state, id) = setup();
        let provider = MockRpc::default();
        runtime().block_on(prepare(&state, &provider, &id)).unwrap();
        provider.final_height.store(101, Ordering::SeqCst);
        if late {
            provider.appear_at_expiry.store(true, Ordering::SeqCst);
        } else {
            provider.status_slot.store(1, Ordering::SeqCst);
        }
        runtime().block_on(sync(&state, &provider, &id)).unwrap();
        assert!(!expired(&operation(&state, &id)));
        assert_eq!(crate::uniswap::terminal(&operation(&state, &id)), late);
        assert!(provider.sent.lock().unwrap().is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn uncertain_native_broadcast_reconciles_without_resigning_or_resending() {
    let (directory, state, id) = setup();
    let provider = MockRpc::default();
    provider.fail_send.store(true, Ordering::SeqCst);
    let prepared = runtime().block_on(prepare(&state, &provider, &id)).unwrap();
    runtime()
        .block_on(submit(&state, &provider, &id, &prepared.tx_hash))
        .unwrap();
    assert_eq!(
        operation(&state, &id).attempts[0].status,
        TransactionStatus::SubmissionUnknown
    );
    let raw = operation(&state, &id).attempts[0].raw_tx.clone();
    runtime()
        .block_on(submit(&state, &provider, &id, &prepared.tx_hash))
        .unwrap();
    assert_eq!(provider.sent.lock().unwrap().len(), 1);
    assert_eq!(operation(&state, &id).attempts[0].raw_tx, raw);
    assert!(crate::uniswap::cancel(&state, &id).is_err());
    std::fs::remove_dir_all(directory).unwrap();
}
