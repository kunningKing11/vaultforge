use std::collections::HashMap;
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use super::*;
use crate::cctp_execution::tests::{execution_state, temp_dir};
use crate::dto::Asset;
use crate::providers::evm::EvmFeeEstimate;
use crate::storage::{decrypt_wallet, derive_storage_key, read_stored_wallet};

const INPUT: &str = "0x1111111111111111111111111111111111111111";
const OUTPUT: &str = "0x2222222222222222222222222222222222222222";

type Hook = Mutex<Option<Box<dyn FnOnce() + Send>>>;
struct MockProvider {
    allowed: AtomicBool,
    output: AtomicU64,
    fee: AtomicU64,
    nonce: AtomicU64,
    fail_broadcast: AtomicBool,
    broadcasts: Mutex<Vec<String>>,
    observations: Mutex<HashMap<String, Result<Option<TxStatus>, String>>>,
    balances: Mutex<(u128, u128, u128)>,
    balance_reads: AtomicU64,
    before_nonce: Hook,
    before_broadcast: Hook,
}

impl Default for MockProvider {
    fn default() -> Self {
        Self {
            allowed: AtomicBool::new(true),
            output: AtomicU64::new(2_000_000),
            fee: AtomicU64::new(42_000),
            nonce: AtomicU64::new(7),
            fail_broadcast: AtomicBool::new(false),
            broadcasts: Mutex::new(vec![]),
            observations: Mutex::new(HashMap::new()),
            balances: Mutex::new((1_000_000, 1_000_000, 0)),
            balance_reads: AtomicU64::new(0),
            before_nonce: Mutex::new(None),
            before_broadcast: Mutex::new(None),
        }
    }
}

impl SwapProvider for MockProvider {
    async fn quote(&self, _: &SwapIntent, fee: Option<u32>) -> Result<PoolQuote, String> {
        Ok(PoolQuote {
            pool_fee: fee.unwrap_or(500),
            amount_out: self.output.load(Ordering::SeqCst).into(),
        })
    }
    async fn allowance(&self, _: &SwapIntent) -> Result<bool, String> {
        Ok(self.allowed.load(Ordering::SeqCst))
    }
    async fn balances(&self, _: &SwapIntent) -> Result<(u128, u128, u128), String> {
        self.balance_reads.fetch_add(1, Ordering::SeqCst);
        Ok(*self.balances.lock().unwrap())
    }
    async fn estimate(&self, _: &SwapIntent, _: &EvmCall) -> Result<EvmCallEstimate, String> {
        let fee = self.fee.load(Ordering::SeqCst);
        Ok(EvmCallEstimate {
            gas_limit: 21_000,
            fees: EvmFeeEstimate {
                max_fee_per_gas: (fee / 21_000).into(),
                max_priority_fee_per_gas: 1,
            },
            max_native_fee: fee.into(),
        })
    }
    async fn nonce(&self, _: &SwapIntent) -> Result<u64, String> {
        if let Some(hook) = self.before_nonce.lock().unwrap().take() {
            hook();
        }
        Ok(self.nonce.load(Ordering::SeqCst))
    }
    async fn observe(&self, _: &SwapIntent, hash: &str) -> Result<Option<TxStatus>, String> {
        self.observations
            .lock()
            .unwrap()
            .get(hash)
            .cloned()
            .unwrap_or(Ok(None))
    }
    async fn broadcast(&self, _: &SwapIntent, raw: &str) -> Result<String, String> {
        if let Some(hook) = self.before_broadcast.lock().unwrap().take() {
            hook();
        }
        self.broadcasts.lock().unwrap().push(raw.into());
        if self.fail_broadcast.load(Ordering::SeqCst) {
            return Err("response lost".into());
        }
        Ok(format!(
            "0x{}",
            hex::encode(Keccak256::digest(
                hex::decode(raw.trim_start_matches("0x")).unwrap()
            ))
        ))
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
}

fn state(directory: &Path) -> Mutex<AppState> {
    let state = execution_state(directory);
    {
        let mut guard = state.lock().unwrap();
        let wallet = guard.wallet.as_mut().unwrap();
        wallet.enabled_networks = vec!["base".into()];
        wallet.assets = [
            (None, "ETH", 18),
            (Some(INPUT), "USDC", 6),
            (Some(OUTPUT), "OUT", 6),
        ]
        .into_iter()
        .map(|(token, symbol, decimals)| Asset {
            symbol: symbol.into(),
            name: symbol.into(),
            unicode_symbol: None,
            balance: "0".into(),
            decimals,
            price_usd: 0.0,
            change_24h: 0.0,
            network: "base".into(),
            token_address: token.map(Into::into),
        })
        .collect();
    }
    state
}

fn create(state: &Mutex<AppState>, provider: &MockProvider) -> String {
    let quote = runtime()
        .block_on(quote_swap(
            state, provider, "base", INPUT, OUTPUT, "1000000", 50,
        ))
        .unwrap();
    create_swap_operation(state, &quote.id)
        .unwrap()
        .operation_id
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

fn observe(provider: &MockProvider, hash: &str, execution: Execution, finality: Finality) {
    provider.observations.lock().unwrap().insert(
        hash.into(),
        Ok(Some(TxStatus {
            execution,
            finality,
            block_height: Some(10),
            block_hash: Some(format!("0x{}", "aa".repeat(32))),
            confirmations: None,
            slot: None,
        })),
    );
}

#[test]
fn approval_confirmation_allows_swap_with_separate_fee_review_before_finality() {
    let directory = temp_dir("swap-approval");
    let state = state(&directory);
    let provider = MockProvider::default();
    provider.allowed.store(false, Ordering::SeqCst);
    let id = create(&state, &provider);
    assert!(
        operation(&state, &id)
            .swap
            .as_ref()
            .unwrap()
            .swap_fee_budget
            .is_none()
    );
    let approval = runtime()
        .block_on(prepare_swap(&state, &provider, &id))
        .unwrap();
    assert_eq!(approval.step, "approval");
    runtime()
        .block_on(submit_swap(&state, &provider, &id, &approval.tx_hash))
        .unwrap();
    observe(
        &provider,
        &approval.tx_hash,
        Execution::Succeeded,
        Finality::Included,
    );
    let reads_before = provider.balance_reads.load(Ordering::SeqCst);
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    assert_eq!(provider.balance_reads.load(Ordering::SeqCst), reads_before);
    let ready = get_state(&state).unwrap().operations.remove(0);
    assert_eq!(ready.progress.title, "Approval complete; prepare swap");
    assert!(ready.needs_sync);
    assert!(ready.can_prepare);
    assert_eq!(
        operation(&state, &id).attempts[0]
            .tx_status
            .as_ref()
            .unwrap()
            .finality,
        Finality::Included
    );
    assert_eq!(ready.stage, SwapStage::Swap);
    assert!(ready.prepared.is_none());
    assert!(ready.progress.subtitle.contains("review its network fee"));
    assert_eq!(operation(&state, &id).attempts.len(), 1);
    assert_eq!(provider.broadcasts.lock().unwrap().len(), 1);
    // An included receipt does not bypass the live spending-permission check.
    assert!(
        runtime()
            .block_on(prepare_swap(&state, &provider, &id))
            .is_err()
    );
    assert_eq!(operation(&state, &id).attempts.len(), 1);
    provider.allowed.store(true, Ordering::SeqCst);
    // A lagging nonce RPC must not cause the consumed approval nonce to be reused.
    let swap = runtime()
        .block_on(prepare_swap(&state, &provider, &id))
        .unwrap();
    assert_eq!(swap.step, "swap");
    let op = operation(&state, &id);
    assert_eq!(op.attempts.len(), 2);
    assert_eq!(op.step, OperationStep::Send);
    assert_eq!(op.attempts[1].nonce_or_sequence.as_deref(), Some("8"));
    assert!(
        runtime()
            .block_on(submit_swap(&state, &provider, &id, &swap.tx_hash))
            .is_err()
    );
    assert_eq!(provider.broadcasts.lock().unwrap().len(), 1);
    provider.nonce.store(8, Ordering::SeqCst);
    runtime()
        .block_on(submit_swap(&state, &provider, &id, &swap.tx_hash))
        .unwrap();
    observe(
        &provider,
        &swap.tx_hash,
        Execution::Succeeded,
        Finality::Included,
    );
    let balance_reads_before_receipt = provider.balance_reads.load(Ordering::SeqCst);
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    assert!(terminal(&operation(&state, &id)));
    assert_eq!(
        provider.balance_reads.load(Ordering::SeqCst),
        balance_reads_before_receipt
    );
    let confirmed = get_state(&state).unwrap().operations.remove(0);
    assert_eq!(confirmed.progress.title, "Swap complete");
    assert!(confirmed.needs_sync);
    assert_eq!(confirmed.stage, SwapStage::Done);
    observe(
        &provider,
        &swap.tx_hash,
        Execution::Succeeded,
        Finality::Finalized,
    );
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    assert_eq!(operation(&state, &id).step, OperationStep::Completed);
    assert_eq!(
        get_state(&state).unwrap().operations[0].stage,
        SwapStage::Done
    );
    assert_eq!(
        get_state(&state).unwrap().operations[0].progress.kind,
        "swap"
    );
}

#[test]
fn approval_reorganization_or_failed_execution_blocks_the_next_transaction() {
    for failed in [false, true] {
        let directory = temp_dir("approval-reorg");
        let state = state(&directory);
        let provider = MockProvider::default();
        provider.allowed.store(false, Ordering::SeqCst);
        let id = create(&state, &provider);
        let approval = runtime()
            .block_on(prepare_swap(&state, &provider, &id))
            .unwrap();
        runtime()
            .block_on(submit_swap(&state, &provider, &id, &approval.tx_hash))
            .unwrap();
        observe(
            &provider,
            &approval.tx_hash,
            Execution::Succeeded,
            Finality::Included,
        );
        runtime()
            .block_on(sync_swap(&state, &provider, &id))
            .unwrap();
        assert!(get_state(&state).unwrap().operations[0].can_prepare);
        if failed {
            observe(
                &provider,
                &approval.tx_hash,
                Execution::Failed,
                Finality::Included,
            );
        } else {
            provider
                .observations
                .lock()
                .unwrap()
                .insert(approval.tx_hash.clone(), Ok(None));
        }
        provider.allowed.store(true, Ordering::SeqCst);
        provider.nonce.store(8, Ordering::SeqCst);
        assert!(
            runtime()
                .block_on(prepare_swap(&state, &provider, &id))
                .is_err()
        );
        let view = get_state(&state).unwrap().operations.remove(0);
        assert!(!view.can_prepare);
        assert_eq!(view.stage, SwapStage::Approve);
        let saved = operation(&state, &id);
        assert_eq!(saved.attempts.len(), 1);
        let mut nonces = crate::tx::evm_nonces::EvmNonceManager::default();
        nonces.restore_operations(&[saved.clone()]).unwrap();
        assert!(nonces.reserve(8453, &saved.sender, 8).is_err());
        assert_eq!(provider.broadcasts.lock().unwrap().len(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn cancellation_releases_only_definitely_unsent_nonces() {
    let directory = temp_dir("swap-cancel");
    let state = state(&directory);
    let provider = MockProvider::default();
    let id = create(&state, &provider);
    runtime()
        .block_on(prepare_swap(&state, &provider, &id))
        .unwrap();
    let sender = operation(&state, &id).sender;
    assert!(
        state
            .lock()
            .unwrap()
            .evm_nonces
            .reserve(8453, &sender, 7)
            .is_err()
    );
    cancel(&state, &id).unwrap();
    let op = operation(&state, &id);
    assert_eq!(op.attempts[0].status, TransactionStatus::Superseded);
    let mut guard = state.lock().unwrap();
    assert_eq!(guard.evm_nonces.reserve(8453, &op.sender, 7).unwrap(), 7);
    drop(guard);
    // Recovery cannot resurrect the cancelled reservation.
    let mut manager = crate::tx::evm_nonces::EvmNonceManager::default();
    manager.restore_operations(&[op.clone()]).unwrap();
    assert_eq!(manager.reserve(8453, &op.sender, 7).unwrap(), 7);
}

#[test]
fn cancelling_after_included_approval_keeps_its_receipt_and_allows_a_fresh_quote() {
    let directory = temp_dir("included-approval-cancel");
    let state = state(&directory);
    let provider = MockProvider::default();
    provider.allowed.store(false, Ordering::SeqCst);
    let id = create(&state, &provider);
    let approval = runtime()
        .block_on(prepare_swap(&state, &provider, &id))
        .unwrap();
    runtime()
        .block_on(submit_swap(&state, &provider, &id, &approval.tx_hash))
        .unwrap();
    observe(
        &provider,
        &approval.tx_hash,
        Execution::Succeeded,
        Finality::Included,
    );
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    cancel(&state, &id).unwrap();
    let cancelled = operation(&state, &id);
    assert_eq!(cancelled.step, OperationStep::Cancelled);
    assert_eq!(
        cancelled.attempts[0].tx_status.as_ref().unwrap().finality,
        Finality::Included
    );
    assert_ne!(cancelled.attempts[0].status, TransactionStatus::Superseded);
    provider.allowed.store(true, Ordering::SeqCst);
    provider.nonce.store(8, Ordering::SeqCst);
    let next = create(&state, &provider);
    let prepared = runtime()
        .block_on(prepare_swap(&state, &provider, &next))
        .unwrap();
    assert_eq!(prepared.step, "swap");
    assert_eq!(
        operation(&state, &next).attempts[0]
            .nonce_or_sequence
            .as_deref(),
        Some("8")
    );
    assert_eq!(provider.broadcasts.lock().unwrap().len(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn comparison_includes_lifi_and_preserves_case_sensitive_solana_identity() {
    let mut relay = comparison_quote();
    relay.id = "relay".into();
    let mut lifi = relay.clone();
    lifi.id = "lifi".into();
    lifi.quoted_output = "2000001".into();
    let ranked = collect_provider_quotes(vec![
        ("Relay", Some(Ok(relay.clone()))),
        ("Uniswap V3", Some(Err("no pool".into()))),
        ("LI.FI", Some(Ok(lifi.clone()))),
    ])
    .unwrap();
    assert_eq!(ranked.quotes[0].id, "lifi");
    assert_eq!(ranked.unavailable[0].provider, "Uniswap V3");
    assert_eq!(
        collect_provider_quotes(vec![
            ("Relay", Some(Err("offline".into()))),
            ("LI.FI", Some(Ok(lifi.clone()))),
        ])
        .unwrap()
        .quotes[0]
            .id,
        "lifi"
    );
    for quote in [&mut relay, &mut lifi] {
        quote.network = "solana".into();
        quote.destination_network = "solana".into();
        quote.sender = "CaseSensitiveWallet".into();
        quote.token_in = "CaseSensitiveMint".into();
        quote.token_out = "OtherMint".into();
    }
    assert!(
        collect_provider_quotes(vec![
            ("Relay", Some(Ok(relay.clone()))),
            ("LI.FI", Some(Ok(lifi.clone())))
        ])
        .is_ok()
    );
    lifi.token_in = lifi.token_in.to_ascii_lowercase();
    assert!(
        collect_provider_quotes(vec![("Relay", Some(Ok(relay))), ("LI.FI", Some(Ok(lifi)))])
            .is_err()
    );
}

#[test]
fn comparison_keeps_either_working_provider_and_reports_both_failures() {
    let quote = comparison_quote();
    let relay_only =
        collect_quotes(Ok(quote.clone()), Some(Err("no Uniswap pool".into()))).unwrap();
    assert_eq!(relay_only.quotes.len(), 1);
    assert_eq!(relay_only.unavailable[0].provider, "Uniswap V3");
    let direct_only = collect_quotes(Err("Relay unavailable".into()), Some(Ok(quote))).unwrap();
    assert_eq!(direct_only.quotes.len(), 1);
    assert_eq!(direct_only.unavailable[0].provider, "Relay");
    let error = collect_quotes(Err("offline".into()), Some(Err("no pool".into())))
        .err()
        .unwrap();
    assert!(error.contains("Relay: offline"));
    assert!(error.contains("Uniswap V3: no pool"));
}

#[test]
fn comparison_ranks_exact_output_in_both_directions_and_keeps_ties_stable() {
    let mut relay = comparison_quote();
    relay.id = "relay".into();
    relay.quoted_output = "100000000000000000000000000001".into();
    let mut direct = relay.clone();
    direct.id = "direct".into();
    direct.quoted_output = "100000000000000000000000000002".into();
    let ranked = collect_quotes(Ok(relay.clone()), Some(Ok(direct.clone()))).unwrap();
    assert_eq!(ranked.quotes[0].id, "direct");
    direct.quoted_output = "100000000000000000000000000000".into();
    let ranked = collect_quotes(Ok(relay.clone()), Some(Ok(direct.clone()))).unwrap();
    assert_eq!(ranked.quotes[0].id, "relay");
    direct.quoted_output = relay.quoted_output.clone();
    assert_eq!(
        collect_quotes(Ok(relay), Some(Ok(direct))).unwrap().quotes[0].id,
        "relay"
    );
}

#[test]
fn comparison_rejects_different_trade_terms() {
    let quote = comparison_quote();
    for mutation in 0..7 {
        let mut other = quote.clone();
        match mutation {
            0 => other.destination_network = "ethereum".into(),
            1 => other.token_out = INPUT.into(),
            2 => other.amount_base_units = "999999".into(),
            3 => other.output_decimals = 18,
            4 => other.input_decimals = 18,
            5 => other.slippage_bps = 100,
            _ => other.sender = INPUT.into(),
        }
        assert!(collect_quotes(Ok(quote.clone()), Some(Ok(other))).is_err());
    }
}

#[test]
fn direct_quotes_only_run_for_same_chain_erc20_pairs() {
    let networks = [
        "arbitrum_one",
        "avalanche_c",
        "base",
        "bnb",
        "ethereum",
        "monad",
        "optimism",
        "polygon",
    ];
    for source in networks {
        for destination in networks {
            assert_eq!(
                direct_quote_eligible(source, destination, INPUT, OUTPUT),
                source == destination
            );
        }
        assert!(!direct_quote_eligible(
            source,
            source,
            "0x0000000000000000000000000000000000000000",
            OUTPUT
        ));
    }
    assert!(!direct_quote_eligible("solana", "solana", INPUT, OUTPUT));
}

#[test]
fn direct_routes_prepare_the_pinned_deployment_on_each_evm_network() {
    for network in [
        "arbitrum_one",
        "avalanche_c",
        "base",
        "bnb",
        "ethereum",
        "monad",
        "optimism",
        "polygon",
    ] {
        let directory = temp_dir("swap-comparison");
        let state = state(&directory);
        {
            let mut guard = state.lock().unwrap();
            let wallet = guard.wallet.as_mut().unwrap();
            wallet.enabled_networks = vec![network.into()];
            for asset in &mut wallet.assets {
                asset.network = network.into();
            }
        }
        let provider = MockProvider::default();
        let quote = runtime()
            .block_on(quote_swap(
                &state, &provider, network, INPUT, OUTPUT, "1000000", 50,
            ))
            .unwrap();
        let created = create_swap_operation(&state, &quote.id).unwrap();
        let prepared = runtime()
            .block_on(prepare_swap(&state, &provider, &created.operation_id))
            .unwrap();
        assert_eq!(prepared.network, network);
        let saved = operation(&state, &created.operation_id);
        let terms = saved.swap.as_ref().unwrap();
        let deployment = deployment_for_network(network).unwrap();
        assert_eq!(terms.chain_id, Some(deployment.chain_id));
        assert_eq!(terms.router, deployment.router);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn encrypted_attempt_survives_restart_and_preparation_is_idempotent() {
    let directory = temp_dir("swap-restart");
    let state = state(&directory);
    let provider = MockProvider::default();
    let password = "test-password";
    let (key, salt) = derive_storage_key(password, Some(&[2; 16])).unwrap();
    {
        let mut guard = state.lock().unwrap();
        guard.encryption_key = Some(key);
        guard.storage_salt = Some(salt);
    }
    let id = create(&state, &provider);
    let prepared = runtime()
        .block_on(prepare_swap(&state, &provider, &id))
        .unwrap();
    let op = operation(&state, &id);
    validate_attempt(&op, &op.attempts[0]).unwrap();
    let stored = read_stored_wallet(&directory.join("wallet.json"))
        .unwrap()
        .unwrap();
    assert_eq!(stored.version, 15);
    let decrypted = decrypt_wallet(&stored, password).unwrap();
    assert_eq!(decrypted.wallet().operations[0], op);
    let (wallet, key, salt) = decrypted.into_parts();
    let mut restored = AppState::from_storage(directory.join("wallet.json"));
    restored.wallet = Some(wallet);
    restored.encryption_key = Some(key);
    restored.storage_salt = Some(salt);
    restored.locked = false;
    restored
        .evm_nonces
        .restore_operations(&[op.clone()])
        .unwrap();
    let restored = Mutex::new(restored);
    let again = runtime()
        .block_on(prepare_swap(&restored, &provider, &id))
        .unwrap();
    assert_eq!(again.tx_hash, prepared.tx_hash);
    assert_eq!(operation(&restored, &id).attempts, op.attempts);
    assert!(provider.broadcasts.lock().unwrap().is_empty());
}

#[test]
fn failed_saves_roll_back_attempts_and_prevent_broadcast() {
    let directory = temp_dir("swap-save-failure");
    let state = state(&directory);
    let provider = MockProvider::default();
    let id = create(&state, &provider);
    state.lock().unwrap().storage_path = directory.clone();
    assert!(
        runtime()
            .block_on(prepare_swap(&state, &provider, &id))
            .is_err()
    );
    assert!(operation(&state, &id).attempts.is_empty());
    state.lock().unwrap().storage_path = directory.join("wallet.json");
    let prepared = runtime()
        .block_on(prepare_swap(&state, &provider, &id))
        .unwrap();
    assert_eq!(
        operation(&state, &id).attempts[0]
            .nonce_or_sequence
            .as_deref(),
        Some("7")
    );
    state.lock().unwrap().storage_path = directory.clone();
    assert!(
        runtime()
            .block_on(submit_swap(&state, &provider, &id, &prepared.tx_hash))
            .is_err()
    );
    assert_eq!(
        operation(&state, &id).attempts[0].status,
        TransactionStatus::Signed
    );
    assert!(provider.broadcasts.lock().unwrap().is_empty());
}

#[test]
fn finalized_failure_is_terminal_but_included_failure_keeps_protection() {
    let directory = temp_dir("swap-failure");
    let state = state(&directory);
    let provider = MockProvider::default();
    let id = create(&state, &provider);
    let prepared = runtime()
        .block_on(prepare_swap(&state, &provider, &id))
        .unwrap();
    runtime()
        .block_on(submit_swap(&state, &provider, &id, &prepared.tx_hash))
        .unwrap();
    observe(
        &provider,
        &prepared.tx_hash,
        Execution::Failed,
        Finality::Included,
    );
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    assert!(!terminal(&operation(&state, &id)));
    assert!(cancel(&state, &id).is_err());
    observe(
        &provider,
        &prepared.tx_hash,
        Execution::Failed,
        Finality::Finalized,
    );
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    assert!(terminal(&operation(&state, &id)));
    assert_eq!(
        swap_activity(&operation(&state, &id)).unwrap().status,
        "failed"
    );
    assert!(
        runtime()
            .block_on(prepare_swap(&state, &provider, &id))
            .is_err()
    );
}

#[test]
fn insufficient_live_balances_and_higher_fees_prevent_signing() {
    for scenario in ["native", "token", "fee"] {
        let directory = temp_dir("swap-funds");
        let state = state(&directory);
        let provider = MockProvider::default();
        let id = create(&state, &provider);
        match scenario {
            "native" => provider.balances.lock().unwrap().0 = 0,
            "token" => provider.balances.lock().unwrap().1 = 0,
            _ => provider.fee.store(63_000, Ordering::SeqCst),
        }
        assert!(
            runtime()
                .block_on(prepare_swap(&state, &provider, &id))
                .is_err(),
            "{scenario}"
        );
        assert!(operation(&state, &id).attempts.is_empty());
    }
}

#[test]
fn legacy_operations_remain_readable_without_swap_fields() {
    let operation = crate::tests::sample_operation();
    let mut json = serde_json::to_value(&operation).unwrap();
    json.as_object_mut().unwrap().remove("swap");
    json.as_object_mut().unwrap().remove("kind");
    let restored: Operation = serde_json::from_value(json).unwrap();
    assert_eq!(restored.kind, OperationKind::Cctp);
    assert!(restored.swap.is_none());
}

#[test]
fn lifi_uses_saved_intent_through_native_evm_signing_and_finality() {
    let directory = temp_dir("lifi-evm-lifecycle");
    let state = state(&directory);
    let provider = MockProvider::default();
    let mut quote = runtime()
        .block_on(quote_swap(
            &state, &provider, "base", INPUT, OUTPUT, "1000000", 50,
        ))
        .unwrap();
    let mut call = crate::tx::lifi::tests::fixture();
    call.data[4 + 3 * 32..4 + 4 * 32]
        .copy_from_slice(&crate::tx::evm::evm_address_word(&quote.sender).unwrap());
    quote.pool_fee = None;
    quote.api = Some(crate::swap_routes::ApiSwap {
        provider: crate::swap_routes::RouteProvider::Lifi,
        request_id: "lifi-quote".into(),
        route: vec![],
        transaction: crate::swap_routes::RouteTransaction::Evm {
            to: call.to.clone(),
            data: format!("0x{}", hex::encode(&call.data)),
            value: "0".into(),
        },
    });
    state
        .lock()
        .unwrap()
        .uniswap_quotes
        .insert(quote.id.clone(), quote.clone());
    let id = create_swap_operation(&state, &quote.id)
        .unwrap()
        .operation_id;
    let prepared = runtime()
        .block_on(prepare_swap(&state, &provider, &id))
        .unwrap();
    let op = operation(&state, &id);
    let verified = validate_attempt(&op, &op.attempts[0]).unwrap();
    assert_eq!(verified.to, call.to);
    assert_eq!(verified.data, call.data);
    assert!(provider.broadcasts.lock().unwrap().is_empty());
    runtime()
        .block_on(submit_swap(&state, &provider, &id, &prepared.tx_hash))
        .unwrap();
    assert_eq!(provider.broadcasts.lock().unwrap().len(), 1);
    observe(
        &provider,
        &prepared.tx_hash,
        Execution::Succeeded,
        Finality::Included,
    );
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    assert!(terminal(&operation(&state, &id)));
    observe(
        &provider,
        &prepared.tx_hash,
        Execution::Succeeded,
        Finality::Finalized,
    );
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    assert_eq!(operation(&state, &id).step, OperationStep::Completed);
    assert_eq!(provider.broadcasts.lock().unwrap().len(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn local_swap_inclusion_completes_the_view_and_reorg_restores_pending_nonce_protection() {
    let directory = temp_dir("included-swap-reorg");
    let state = state(&directory);
    let provider = MockProvider::default();
    let id = create(&state, &provider);
    let prepared = runtime()
        .block_on(prepare_swap(&state, &provider, &id))
        .unwrap();
    runtime()
        .block_on(submit_swap(&state, &provider, &id, &prepared.tx_hash))
        .unwrap();
    observe(
        &provider,
        &prepared.tx_hash,
        Execution::Succeeded,
        Finality::Included,
    );
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    let view = get_state(&state).unwrap().operations.remove(0);
    assert_eq!(view.stage, SwapStage::Done);
    assert_eq!(view.progress.title, "Swap complete");
    assert!(view.terminal && view.needs_sync);
    assert!(!view.can_prepare);
    let included = operation(&state, &id);
    assert_eq!(included.step, OperationStep::Send);
    assert_eq!(
        included.attempts[0].tx_status.as_ref().unwrap().finality,
        Finality::Included
    );
    let mut restored = crate::tx::evm_nonces::EvmNonceManager::default();
    restored.restore_operations(&[included.clone()]).unwrap();
    assert_eq!(restored.reserve(8453, &included.sender, 7).unwrap(), 8);
    assert!(
        runtime()
            .block_on(prepare_swap(&state, &provider, &id))
            .is_err()
    );
    provider
        .observations
        .lock()
        .unwrap()
        .insert(prepared.tx_hash.clone(), Ok(None));
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    let pending = get_state(&state).unwrap().operations.remove(0);
    assert!(!pending.terminal);
    assert!(pending.needs_sync);
    assert_eq!(pending.stage, SwapStage::Swap);
    let reverted = operation(&state, &id);
    restored.restore_operations(&[reverted.clone()]).unwrap();
    assert!(restored.reserve(8453, &reverted.sender, 8).is_err());
    observe(
        &provider,
        &prepared.tx_hash,
        Execution::Succeeded,
        Finality::Finalized,
    );
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    let final_view = get_state(&state).unwrap().operations.remove(0);
    assert!(final_view.terminal);
    assert!(!final_view.needs_sync);
    assert_eq!(operation(&state, &id).step, OperationStep::Completed);
    assert_eq!(provider.broadcasts.lock().unwrap().len(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn local_swap_inclusion_permits_the_next_swap_without_waiting_for_finality() {
    let directory = temp_dir("included-swap-next");
    let state = state(&directory);
    let provider = MockProvider::default();
    let id = create(&state, &provider);
    let prepared = runtime()
        .block_on(prepare_swap(&state, &provider, &id))
        .unwrap();
    runtime()
        .block_on(submit_swap(&state, &provider, &id, &prepared.tx_hash))
        .unwrap();
    observe(
        &provider,
        &prepared.tx_hash,
        Execution::Succeeded,
        Finality::Included,
    );
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    let next = create(&state, &provider);
    assert_ne!(next, id);
    let next_tx = runtime()
        .block_on(prepare_swap(&state, &provider, &next))
        .unwrap();
    assert_ne!(next_tx.tx_hash, prepared.tx_hash);
    assert_eq!(
        operation(&state, &next).attempts[0]
            .nonce_or_sequence
            .as_deref(),
        Some("8")
    );
    assert_eq!(provider.broadcasts.lock().unwrap().len(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn lost_response_is_reconciled_without_automatic_rebroadcast() {
    let directory = temp_dir("swap-lost-response");
    let state = Arc::new(state(&directory));
    let provider = MockProvider::default();
    let id = create(&state, &provider);
    let prepared = runtime()
        .block_on(prepare_swap(&state, &provider, &id))
        .unwrap();
    let checked = state.clone();
    let checked_id = id.clone();
    *provider.before_broadcast.lock().unwrap() = Some(Box::new(move || {
        let guard = checked.lock().unwrap();
        let attempt = guard
            .wallet
            .as_ref()
            .unwrap()
            .operations
            .iter()
            .find(|o| o.id == checked_id)
            .unwrap()
            .attempts
            .last()
            .unwrap();
        assert_eq!(attempt.status, TransactionStatus::SubmissionUnknown);
        assert!(attempt.raw_tx.is_some());
        let stored = read_stored_wallet(&guard.storage_path).unwrap().unwrap();
        assert_eq!(stored.version, 15);
    }));
    provider.fail_broadcast.store(true, Ordering::SeqCst);
    runtime()
        .block_on(submit_swap(&state, &provider, &id, &prepared.tx_hash))
        .unwrap();
    assert_eq!(
        operation(&state, &id).attempts[0].status,
        TransactionStatus::SubmissionUnknown
    );
    assert!(cancel(&state, &id).is_err());
    assert!(
        runtime()
            .block_on(prepare_swap(&state, &provider, &id))
            .is_err()
    );
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    assert_eq!(provider.broadcasts.lock().unwrap().len(), 1);
    observe(
        &provider,
        &prepared.tx_hash,
        Execution::Succeeded,
        Finality::Finalized,
    );
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    assert_eq!(operation(&state, &id).step, OperationStep::Completed);
    runtime()
        .block_on(submit_swap(&state, &provider, &id, &prepared.tx_hash))
        .unwrap();
    assert_eq!(provider.broadcasts.lock().unwrap().len(), 1);
}

#[test]
fn native_max_reserves_fees_without_rounding_or_unsigned_underflow() {
    assert_eq!(
        native_spendable(100_000_000_000_000_001, 42_000).unwrap(),
        99_999_999_999_958_001
    );
    assert_eq!(native_spendable(u128::MAX, 1).unwrap(), u128::MAX - 1);
    assert!(native_spendable(42_000, 42_000).is_err());
    assert!(native_spendable(41_999, 42_000).is_err());
    assert!(native_spendable(0, 0).is_err());
    assert!(native_spendable(100, 0).is_err());
}

#[test]
fn quote_is_read_only_and_creation_reuses_its_backend_bound_terms() {
    let directory = temp_dir("swap-quote-binding");
    let state = state(&directory);
    let provider = MockProvider::default();
    let quote = runtime()
        .block_on(quote_swap(
            &state, &provider, "base", INPUT, OUTPUT, "1000000", 50,
        ))
        .unwrap();
    assert!(!directory.join("wallet.json").exists());
    assert!(
        state
            .lock()
            .unwrap()
            .wallet
            .as_ref()
            .unwrap()
            .operations
            .is_empty()
    );
    let first = create_swap_operation(&state, &quote.id).unwrap();
    let second = create_swap_operation(&state, &quote.id).unwrap();
    assert_eq!(first.operation_id, second.operation_id);
    assert_eq!(
        state
            .lock()
            .unwrap()
            .wallet
            .as_ref()
            .unwrap()
            .operations
            .len(),
        1
    );
    assert!(create_swap_operation(&state, "altered-quote-id").is_err());
    assert_eq!(
        operation(&state, &first.operation_id)
            .swap
            .unwrap()
            .minimum_received,
        "1990000"
    );
}

fn comparison_quote() -> UniswapQuote {
    let directory = temp_dir("swap-comparison");
    let state = state(&directory);
    let quote = runtime()
        .block_on(quote_swap(
            &state,
            &MockProvider::default(),
            "base",
            INPUT,
            OUTPUT,
            "1000000",
            50,
        ))
        .unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    quote
}

#[test]
fn rejects_changed_generation_and_operation_snapshot_before_signing() {
    for scenario in ["generation", "snapshot"] {
        let directory = temp_dir("swap-stale");
        let state = Arc::new(state(&directory));
        let provider = MockProvider::default();
        let id = create(&state, &provider);
        let changed = state.clone();
        let changed_id = id.clone();
        *provider.before_nonce.lock().unwrap() = Some(Box::new(move || {
            let mut guard = changed.lock().unwrap();
            if scenario == "generation" {
                guard.advance_wallet_generation();
            } else {
                guard
                    .wallet
                    .as_mut()
                    .unwrap()
                    .operations
                    .iter_mut()
                    .find(|o| o.id == changed_id)
                    .unwrap()
                    .last_error = Some("changed".into());
            }
        }));
        assert!(
            runtime()
                .block_on(prepare_swap(&state, &provider, &id))
                .is_err()
        );
        assert!(operation(&state, &id).attempts.is_empty());
    }
}

#[test]
fn rejects_stale_quotes_expiry_and_wrong_hash_before_broadcast() {
    for scenario in ["price", "expiry", "hash", "status", "nonce"] {
        let directory = temp_dir("swap-requote");
        let state = state(&directory);
        let provider = MockProvider::default();
        let id = create(&state, &provider);
        let prepared = runtime()
            .block_on(prepare_swap(&state, &provider, &id))
            .unwrap();
        match scenario {
            "price" => provider.output.store(1, Ordering::SeqCst),
            "expiry" => {
                state.lock().unwrap().wallet.as_mut().unwrap().operations[0]
                    .swap
                    .as_mut()
                    .unwrap()
                    .deadline = now() - 1
            }
            "status" => {
                provider
                    .observations
                    .lock()
                    .unwrap()
                    .insert(prepared.tx_hash.clone(), Err("RPC unavailable".into()));
            }
            "nonce" => provider.nonce.store(8, Ordering::SeqCst),
            _ => {}
        }
        let hash = if scenario == "hash" {
            "different-hash"
        } else {
            &prepared.tx_hash
        };
        assert!(
            runtime()
                .block_on(submit_swap(&state, &provider, &id, hash))
                .is_err(),
            "{scenario}"
        );
        assert!(provider.broadcasts.lock().unwrap().is_empty());
    }
}

#[test]
fn rejects_tampered_signed_bytes_context_and_deployment() {
    for scenario in ["bytes", "context", "router"] {
        let directory = temp_dir("swap-signed-binding");
        let state = state(&directory);
        let provider = MockProvider::default();
        let id = create(&state, &provider);
        let prepared = runtime()
            .block_on(prepare_swap(&state, &provider, &id))
            .unwrap();
        {
            let mut guard = state.lock().unwrap();
            let operation = &mut guard.wallet.as_mut().unwrap().operations[0];
            match scenario {
                "bytes" => operation.attempts[0].raw_tx = Some("0x0200".into()),
                "context" => operation.attempts[0].evm_context.as_mut().unwrap().to = OUTPUT.into(),
                _ => operation.swap.as_mut().unwrap().router = OUTPUT.into(),
            }
        }
        assert!(
            runtime()
                .block_on(submit_swap(&state, &provider, &id, &prepared.tx_hash))
                .is_err(),
            "{scenario}"
        );
        assert!(provider.broadcasts.lock().unwrap().is_empty());
    }
}

#[test]
fn result_save_failure_keeps_the_durable_submission_unknown_marker() {
    let directory = temp_dir("swap-result-save-failure");
    let state = Arc::new(state(&directory));
    let provider = MockProvider::default();
    let password = "marker-test-password";
    let (key, salt) = derive_storage_key(password, Some(&[2; 16])).unwrap();
    {
        let mut guard = state.lock().unwrap();
        guard.encryption_key = Some(key);
        guard.storage_salt = Some(salt);
    }
    let id = create(&state, &provider);
    let prepared = runtime()
        .block_on(prepare_swap(&state, &provider, &id))
        .unwrap();
    let changed = state.clone();
    let failed_path = directory.clone();
    *provider.before_broadcast.lock().unwrap() = Some(Box::new(move || {
        changed.lock().unwrap().storage_path = failed_path;
    }));
    assert!(
        runtime()
            .block_on(submit_swap(&state, &provider, &id, &prepared.tx_hash))
            .is_err()
    );
    assert_eq!(provider.broadcasts.lock().unwrap().len(), 1);
    assert_eq!(
        operation(&state, &id).attempts[0].status,
        TransactionStatus::SubmissionUnknown
    );
    let stored = read_stored_wallet(&directory.join("wallet.json"))
        .unwrap()
        .unwrap();
    let decrypted = decrypt_wallet(&stored, password).unwrap();
    let attempt = &decrypted.wallet().operations[0].attempts[0];
    assert_eq!(attempt.status, TransactionStatus::SubmissionUnknown);
    assert_eq!(attempt.tx_hash, prepared.tx_hash);
    assert_eq!(
        attempt.raw_tx.as_deref(),
        Some(provider.broadcasts.lock().unwrap()[0].as_str())
    );
    state.lock().unwrap().storage_path = directory.join("wallet.json");
    observe(
        &provider,
        &prepared.tx_hash,
        Execution::Succeeded,
        Finality::Finalized,
    );
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    assert_eq!(operation(&state, &id).step, OperationStep::Completed);
    assert_eq!(provider.broadcasts.lock().unwrap().len(), 1);
}

#[test]
fn routed_deposit_stays_pending_until_destination_finality_and_survives_restart() {
    let directory = temp_dir("routed-swap");
    let state = state(&directory);
    let (key, salt) = derive_storage_key("route-password", Some(&[2; 16])).unwrap();
    {
        let mut guard = state.lock().unwrap();
        guard.encryption_key = Some(key);
        guard.storage_salt = Some(salt);
    }
    let provider = MockProvider::default();
    let id = create(&state, &provider);
    let mut op = operation(&state, &id);
    let body: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/relay-base-optimism.json")).unwrap();
    let mut order = body["protocol"]["v2"]["orderData"].clone();
    order["inputs"][0]["payment"]["currency"] = serde_json::json!(INPUT);
    order["inputs"][0]["payment"]["amount"] = serde_json::json!("1000000");
    order["output"]["payments"][0]["currency"] = serde_json::json!(OUTPUT);
    order["output"]["payments"][0]["minimumAmount"] = serde_json::json!("1990000");
    order["output"]["payments"][0]["expectedAmount"] = serde_json::json!("2000000");
    order["output"]["payments"][0]["recipient"] = serde_json::json!(op.sender);
    for refund in order["inputs"][0]["refunds"].as_array_mut().unwrap() {
        refund["recipient"] = serde_json::json!(op.sender);
    }
    let relay = crate::providers::relay::RelayData {
        destination_network: "optimism".into(),
        request_id: body["steps"][0]["requestId"].as_str().unwrap().into(),
        order_id: crate::tx::relay::order_id(&order).unwrap(),
        order,
        route: vec![],
        destination_hash: None,
        destination_status: None,
        status: None,
    };
    let terms = op.swap.as_mut().unwrap();
    terms.pool_fee = None;
    terms.router = crate::providers::relay::DEPOSITORY.into();
    terms.relay = Some(relay);
    op.destination_asset.network = "optimism".into();
    state.lock().unwrap().wallet.as_mut().unwrap().operations[0] = op;
    let prepared = runtime()
        .block_on(prepare_swap(&state, &provider, &id))
        .unwrap();
    let signed = operation(&state, &id);
    validate_attempt(&signed, &signed.attempts[0]).unwrap();
    let mut nonces = crate::tx::evm_nonces::EvmNonceManager::default();
    nonces.restore_operations(&[signed.clone()]).unwrap();
    assert!(nonces.reserve(8453, &signed.sender, 7).is_err());
    runtime()
        .block_on(submit_swap(&state, &provider, &id, &prepared.tx_hash))
        .unwrap();
    observe(
        &provider,
        &prepared.tx_hash,
        Execution::Succeeded,
        Finality::Included,
    );
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    let included = get_state(&state).unwrap().operations.remove(0);
    assert_eq!(included.progress.title, "Swap deposit confirmed");
    assert!(included.progress.subtitle.contains("does not yet prove"));
    assert!(!included.terminal);
    assert!(included.needs_sync);
    observe(
        &provider,
        &prepared.tx_hash,
        Execution::Succeeded,
        Finality::Finalized,
    );
    runtime()
        .block_on(sync_swap(&state, &provider, &id))
        .unwrap();
    let mut saved = operation(&state, &id);
    assert!(!terminal(&saved));
    assert_ne!(saved.step, OperationStep::Completed);
    assert!(get_state(&state).unwrap().operations[0].needs_sync);
    assert_eq!(
        get_state(&state).unwrap().operations[0].stage,
        SwapStage::Receive
    );
    assert!(cancel(&state, &id).is_err());
    assert!(
        runtime()
            .block_on(prepare_swap(&state, &provider, &id))
            .is_err()
    );
    assert_eq!(provider.broadcasts.lock().unwrap().len(), 1);
    let stored = read_stored_wallet(&directory.join("wallet.json"))
        .unwrap()
        .unwrap();
    let restored = decrypt_wallet(&stored, "route-password").unwrap();
    assert_eq!(restored.wallet().operations[0], saved);
    saved = restored.wallet().operations[0].clone();
    validate_operation(&saved).unwrap();
    assert!(saved.swap.as_ref().unwrap().pool_fee.is_none());
    let relay = saved.swap.as_mut().unwrap().relay.as_mut().unwrap();
    relay.status = Some("success".into());
    relay.destination_status = Some(TxStatus::new(Execution::Succeeded, Finality::Included));
    update_progress(&mut saved);
    assert!(!terminal(&saved));
    assert_eq!(swap_stage(&saved), SwapStage::Receive);
    saved
        .swap
        .as_mut()
        .unwrap()
        .relay
        .as_mut()
        .unwrap()
        .destination_status
        .as_mut()
        .unwrap()
        .finality = Finality::Finalized;
    update_progress(&mut saved);
    assert!(terminal(&saved));
    assert_eq!(saved.step, OperationStep::Completed);
}
