use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::{
    CctpPreparedSource, CctpSourceProvider, create_source_operation, prepare_source_step,
    submit_source_step,
};
use crate::cctp::CctpTransfer;
use crate::dto::{AssetIdentity, FiatCurrency, Wallet};
use crate::operations::{
    Operation, OperationStep, TransactionAttempt, TransactionStatus, TransactionStep,
};
use crate::providers::cctp::{CctpPreflight, CctpSourcePreparation, PreparedCctpCall};
use crate::providers::evm::{EvmCallEstimate, EvmFeeEstimate};
use crate::state::AppState;
use crate::tx::cctp::{build_approval_call, build_burn_call};
use crate::tx::evm_nonces::EvmNonceManager;
use crate::tx::tx_status::{Execution, Finality, TxStatus};

const TEST_MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const RECIPIENT: &str = "0x4444444444444444444444444444444444444444";

pub(crate) struct MockSourceProvider {
    pub(crate) preflight: Mutex<Option<Result<CctpPreflight, String>>>,
    pub(crate) pending_nonce: Mutex<u64>,
    pub(crate) observation: Mutex<Option<Result<Option<TxStatus>, String>>>,
    signed_preflight: Mutex<Option<Result<(), String>>>,
    pub(crate) broadcasts: Mutex<VecDeque<Result<String, String>>>,
    pub(crate) broadcast_calls: Mutex<usize>,
    balances: Mutex<Result<(u128, u128), String>>,
}

impl Default for MockSourceProvider {
    fn default() -> Self {
        Self {
            preflight: Mutex::new(None),
            pending_nonce: Mutex::new(0),
            observation: Mutex::new(None),
            signed_preflight: Mutex::new(None),
            broadcasts: Mutex::new(VecDeque::new()),
            broadcast_calls: Mutex::new(0),
            balances: Mutex::new(Ok((0, 0))),
        }
    }
}

impl CctpSourceProvider for MockSourceProvider {
    async fn prepare(&self, _transfer: &CctpTransfer) -> Result<CctpPreflight, String> {
        self.preflight
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| Err("no preflight queued".to_string()))
    }

    async fn nonce(&self, _transfer: &CctpTransfer) -> Result<u64, String> {
        Ok(*self.pending_nonce.lock().unwrap())
    }

    async fn observe(
        &self,
        _transfer: &CctpTransfer,
        _hash: &str,
    ) -> Result<Option<TxStatus>, String> {
        match self.observation.lock().unwrap().clone() {
            Some(result) => result,
            None => Ok(None),
        }
    }

    async fn preflight_signed(
        &self,
        _transfer: &CctpTransfer,
        _attempt: &TransactionAttempt,
    ) -> Result<(), String> {
        self.signed_preflight
            .lock()
            .unwrap()
            .take()
            .unwrap_or(Ok(()))
    }

    async fn broadcast(&self, _transfer: &CctpTransfer, _raw: &str) -> Result<String, String> {
        *self.broadcast_calls.lock().unwrap() += 1;
        self.broadcasts
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err("no broadcast queued".to_string()))
    }

    async fn balances(&self, _transfer: &CctpTransfer) -> Result<(u128, u128), String> {
        self.balances.lock().unwrap().clone()
    }
}

fn test_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
}

fn test_address() -> String {
    let key = crate::derivation::signing_key_from_mnemonic(TEST_MNEMONIC, "").unwrap();
    let bytes: [u8; 32] = key.to_bytes().into();
    crate::derivation::ethereum_address_from_private_key(&bytes).unwrap()
}

pub(crate) fn temp_dir(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "vaultforge-{name}-{}",
        crate::activity::random_hex(16)
    ));
    std::fs::create_dir(&directory).unwrap();
    directory
}

pub(crate) fn execution_state(directory: &Path) -> Mutex<AppState> {
    let mut state = AppState::from_storage(directory.join("wallet.json"));
    let mut addresses = HashMap::new();
    addresses.insert("evm".to_string(), test_address());
    state.wallet = Some(Wallet {
        name: "CCTP execution fixture".to_string(),
        mnemonic: TEST_MNEMONIC.to_string(),
        bip39_passphrase: String::new(),
        created_at: "2026-10-08T00:00:00Z".to_string(),
        addresses,
        wallet_password_hash: "test hash".to_string(),
        fiat_currency: FiatCurrency::Usd,
        usd_exchange_rate: 1.0,
        assets: vec![],
        activity: vec![],
        operations: vec![],
        enabled_networks: vec!["ethereum".to_string(), "arbitrum_one".to_string()],
        auto_lock_timeout_secs: None,
        use_crypto_symbols: false,
        hide_below_usd: crate::dto::default_hide_below_usd(),
    });
    state.encryption_key = Some([9; 32]);
    state.storage_salt = Some(vec![2; 16]);
    state.wallet_generation = 3;
    state.locked = false;
    Mutex::new(state)
}

pub(crate) fn source_asset() -> AssetIdentity {
    AssetIdentity::new(
        "ethereum",
        Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
    )
}

pub(crate) fn destination_asset() -> AssetIdentity {
    AssetIdentity::new(
        "arbitrum_one",
        Some("0xaf88d065e77c8cC2239327C5EDb3A432268e5831"),
    )
}

fn create_operation(state: &Mutex<AppState>) -> String {
    create_source_operation(
        state,
        source_asset(),
        destination_asset(),
        RECIPIENT.to_string(),
        "1000000".to_string(),
    )
    .unwrap()
    .operation_id
}

fn operation_transfer(state: &Mutex<AppState>) -> CctpTransfer {
    state.lock().unwrap().wallet.as_ref().unwrap().operations[0]
        .cctp_transfer()
        .unwrap()
}

fn fee_estimate() -> EvmCallEstimate {
    EvmCallEstimate {
        gas_limit: 21_000,
        fees: EvmFeeEstimate {
            max_priority_fee_per_gas: 1,
            max_fee_per_gas: 2,
        },
        max_native_fee: 42_000,
    }
}

pub(crate) fn approval_preflight(transfer: &CctpTransfer) -> CctpPreflight {
    CctpPreflight {
        source: CctpSourcePreparation::Approval {
            approval: PreparedCctpCall {
                call: build_approval_call(transfer).unwrap(),
                estimate: fee_estimate(),
            },
            provisional_burn_fee: 50_000,
        },
        source_native_budget: 92_000,
        provisional_destination_fee: 84_000,
        protocol_fee: 0,
        expected_receive_amount: transfer.amount(),
    }
}

pub(crate) fn burn_preflight(transfer: &CctpTransfer) -> CctpPreflight {
    CctpPreflight {
        source: CctpSourcePreparation::Burn(PreparedCctpCall {
            call: build_burn_call(transfer).unwrap(),
            estimate: fee_estimate(),
        }),
        source_native_budget: 42_000,
        provisional_destination_fee: 84_000,
        protocol_fee: 0,
        expected_receive_amount: transfer.amount(),
    }
}

fn prepare_approval(
    state: &Mutex<AppState>,
    provider: &MockSourceProvider,
    id: &str,
) -> CctpPreparedSource {
    let transfer = operation_transfer(state);
    provider
        .preflight
        .lock()
        .unwrap()
        .replace(Ok(approval_preflight(&transfer)));
    provider
        .pending_nonce
        .lock()
        .map(|mut nonce| *nonce = 7)
        .unwrap();
    test_runtime()
        .block_on(prepare_source_step(state, provider, id))
        .unwrap()
}

fn last_attempt(state: &Mutex<AppState>) -> TransactionAttempt {
    state.lock().unwrap().wallet.as_ref().unwrap().operations[0]
        .attempts
        .last()
        .unwrap()
        .clone()
}

fn operations(state: &Mutex<AppState>) -> Vec<Operation> {
    state
        .lock()
        .unwrap()
        .wallet
        .as_ref()
        .unwrap()
        .operations
        .clone()
}

fn block_err<T>(result: Result<T, String>) -> String {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(error) => error,
    }
}

#[test]
fn advances_to_burn_only_after_confirmed_approval() {
    let directory = temp_dir("cctp-approval-then-burn");
    let state = execution_state(&directory);
    let id = create_operation(&state);
    let provider = MockSourceProvider::default();
    let prepared = prepare_approval(&state, &provider, &id);

    provider
        .broadcasts
        .lock()
        .unwrap()
        .push_back(Ok(prepared.tx_hash.clone()));
    test_runtime()
        .block_on(submit_source_step(
            &state,
            &provider,
            &id,
            &prepared.tx_hash,
        ))
        .unwrap();
    // A broadcast hash is acceptance, not execution success.
    assert_eq!(last_attempt(&state).status, TransactionStatus::Submitted);

    provider
        .observation
        .lock()
        .unwrap()
        .replace(Ok(Some(TxStatus::new(
            Execution::Succeeded,
            Finality::Finalized,
        ))));
    let transfer = operation_transfer(&state);
    provider
        .preflight
        .lock()
        .unwrap()
        .replace(Ok(burn_preflight(&transfer)));
    *provider.pending_nonce.lock().unwrap() = 8;
    let prepared = test_runtime()
        .block_on(prepare_source_step(&state, &provider, &id))
        .unwrap();
    assert_eq!(prepared.step, "burn");
    assert_eq!(
        state.lock().unwrap().wallet.as_ref().unwrap().operations[0].step,
        OperationStep::Burn
    );
    assert_eq!(last_attempt(&state).step, TransactionStep::Burn);

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn blocks_conflicting_actions_and_duplicate_creation() {
    let directory = temp_dir("cctp-duplicate");
    let state = execution_state(&directory);
    let id = create_operation(&state);

    assert!(
        create_source_operation(
            &state,
            source_asset(),
            destination_asset(),
            RECIPIENT.to_string(),
            "1000000".to_string(),
        )
        .is_err()
    );

    let provider = MockSourceProvider::default();
    {
        let mut guard = state.lock().unwrap();
        let key = (
            guard.wallet_generation,
            format!("1:{}", test_address().to_ascii_lowercase()),
        );
        guard.cctp_actions.insert(key);
    }
    assert!(
        test_runtime()
            .block_on(prepare_source_step(&state, &provider, &id))
            .is_err()
    );

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn blocks_ordinary_send_nonce_on_unresolved_operation() {
    let directory = temp_dir("cctp-nonce-conflict");
    let state = execution_state(&directory);
    let id = create_operation(&state);
    let provider = MockSourceProvider::default();
    prepare_approval(&state, &provider, &id);

    let mut guard = state.lock().unwrap();
    assert!(guard.evm_nonces.reserve(1, &test_address(), 7).is_err());
    assert!(
        guard
            .evm_nonces
            .reserve(1, "0x9999999999999999999999999999999999999999", 7)
            .is_ok()
    );
    drop(guard);

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn persists_signed_attempt_before_broadcast_and_survives_lost_response() {
    let directory = temp_dir("cctp-lost-response");
    let state = execution_state(&directory);
    let id = create_operation(&state);
    let provider = MockSourceProvider::default();
    let prepared = prepare_approval(&state, &provider, &id);
    assert_eq!(last_attempt(&state).status, TransactionStatus::Signed);

    let hash = prepared.tx_hash;
    provider
        .broadcasts
        .lock()
        .unwrap()
        .push_back(Err("transport down".to_string()));
    let error =
        block_err(test_runtime().block_on(submit_source_step(&state, &provider, &id, &hash)));
    assert!(error.contains("uncertain"));
    let attempt = last_attempt(&state);
    assert_eq!(attempt.status, TransactionStatus::SubmissionUnknown);
    assert!(attempt.raw_tx.is_some());

    provider
        .observation
        .lock()
        .unwrap()
        .replace(Ok(Some(TxStatus::new(
            Execution::Succeeded,
            Finality::Finalized,
        ))));
    test_runtime()
        .block_on(submit_source_step(&state, &provider, &id, &hash))
        .unwrap();
    assert_eq!(*provider.broadcast_calls.lock().unwrap(), 1);
    assert_eq!(last_attempt(&state).status, TransactionStatus::Confirmed);

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn reconciliation_failure_blocks_submit_with_provider_detail() {
    let directory = temp_dir("cctp-reconcile-block");
    let state = execution_state(&directory);
    let id = create_operation(&state);
    let provider = MockSourceProvider::default();
    let prepared = prepare_approval(&state, &provider, &id);

    provider.observation.lock().unwrap().replace(Err(
        "RPC request failed (attempt 3/3): connection refused".to_string(),
    ));
    let error = block_err(test_runtime().block_on(submit_source_step(
        &state,
        &provider,
        &id,
        &prepared.tx_hash,
    )));
    assert!(error.contains("Reconciliation failed"), "{error}");
    assert!(error.contains("Status check failed"), "{error}");
    assert!(error.contains("Nothing was broadcast"), "{error}");
    assert_eq!(*provider.broadcast_calls.lock().unwrap(), 0);
    assert_eq!(last_attempt(&state).status, TransactionStatus::Signed);

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn recovered_reconciliation_unblocks_submit() {
    let directory = temp_dir("cctp-reconcile-retry");
    let state = execution_state(&directory);
    let id = create_operation(&state);
    let provider = MockSourceProvider::default();
    let prepared = prepare_approval(&state, &provider, &id);

    provider
        .observation
        .lock()
        .unwrap()
        .replace(Err("RPC request failed (attempt 3/3): timeout".to_string()));
    block_err(test_runtime().block_on(submit_source_step(
        &state,
        &provider,
        &id,
        &prepared.tx_hash,
    )));

    provider.observation.lock().unwrap().replace(Ok(None));
    provider
        .broadcasts
        .lock()
        .unwrap()
        .push_back(Ok(prepared.tx_hash.clone()));
    test_runtime()
        .block_on(submit_source_step(
            &state,
            &provider,
            &id,
            &prepared.tx_hash,
        ))
        .unwrap();
    assert_eq!(last_attempt(&state).status, TransactionStatus::Submitted);

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn recovers_unresolved_nonce_after_restart() {
    let directory = temp_dir("cctp-restart");
    let state = execution_state(&directory);
    let id = create_operation(&state);
    let provider = MockSourceProvider::default();
    prepare_approval(&state, &provider, &id);

    let mut restored = EvmNonceManager::default();
    restored.restore_operations(&operations(&state)).unwrap();
    assert!(restored.reserve(1, &test_address(), 7).is_err());

    let mut empty = EvmNonceManager::default();
    empty.restore_operations(&[]).unwrap();
    assert!(empty.reserve(1, &test_address(), 7).is_ok());

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn rejects_definitive_preflight_without_broadcasting() {
    let directory = temp_dir("cctp-definitive-rejection");
    let state = execution_state(&directory);
    let id = create_operation(&state);
    let provider = MockSourceProvider::default();
    let prepared = prepare_approval(&state, &provider, &id);
    provider
        .signed_preflight
        .lock()
        .unwrap()
        .replace(Err("destination simulation reverted".to_string()));

    assert!(
        test_runtime()
            .block_on(submit_source_step(
                &state,
                &provider,
                &id,
                &prepared.tx_hash
            ))
            .is_err()
    );
    assert_eq!(*provider.broadcast_calls.lock().unwrap(), 0);
    assert_eq!(last_attempt(&state).status, TransactionStatus::Signed);

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn rejects_locked_or_replaced_wallet() {
    let directory = temp_dir("cctp-session-change");
    let state = execution_state(&directory);
    let id = create_operation(&state);
    let provider = MockSourceProvider::default();
    let prepared = prepare_approval(&state, &provider, &id);

    {
        let mut guard = state.lock().unwrap();
        guard.wallet.as_mut().unwrap().addresses.insert(
            "evm".to_string(),
            "0x5555555555555555555555555555555555555555".to_string(),
        );
    }
    assert!(
        test_runtime()
            .block_on(submit_source_step(
                &state,
                &provider,
                &id,
                &prepared.tx_hash
            ))
            .is_err()
    );
    assert_eq!(*provider.broadcast_calls.lock().unwrap(), 0);

    state.lock().unwrap().locked = true;
    assert!(
        test_runtime()
            .block_on(prepare_source_step(&state, &provider, &id))
            .is_err()
    );

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn save_failure_blocks_broadcast() {
    let directory = temp_dir("cctp-save-failure");
    let state = execution_state(&directory);
    let id = create_operation(&state);
    let provider = MockSourceProvider::default();
    let prepared = prepare_approval(&state, &provider, &id);

    state.lock().unwrap().storage_path = directory.clone();
    assert!(
        test_runtime()
            .block_on(submit_source_step(
                &state,
                &provider,
                &id,
                &prepared.tx_hash
            ))
            .is_err()
    );
    assert_eq!(*provider.broadcast_calls.lock().unwrap(), 0);
    assert_eq!(last_attempt(&state).status, TransactionStatus::Signed);

    std::fs::remove_dir_all(directory).unwrap();
}
