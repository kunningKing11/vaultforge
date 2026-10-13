use std::sync::Mutex;

use sha3::{Digest, Keccak256};

use super::{CctpMintProvider, prepare_mint, submit_mint, sync_cctp};
use crate::cctp_execution::{
    create_source_operation,
    tests::{destination_asset, execution_state, source_asset, temp_dir},
};
use crate::operations::{
    MintEvidence, Operation, OperationStep, TransactionAttempt, TransactionStatus, TransactionStep,
};
use crate::providers::cctp::PreparedCctpCall;
use crate::providers::evm::{EvmCallEstimate, EvmFeeEstimate};
use crate::state::AppState;
use crate::tx::tx_status::{Execution, Finality, TxStatus};

#[derive(Default)]
struct MockMint {
    consumed: Mutex<bool>,
    proof: Mutex<Option<MintEvidence>>,
    broadcasts: Mutex<usize>,
    lost_response: bool,
    nonce: u64,
    preflight_error: bool,
}
impl CctpMintProvider for MockMint {
    async fn observe(
        &self,
        operation: &Operation,
        destination: bool,
        _hash: &str,
    ) -> Result<Option<TxStatus>, String> {
        if destination {
            Ok(self
                .proof
                .lock()
                .unwrap()
                .as_ref()
                .map(|p| p.observation.clone()))
        } else {
            Ok(operation
                .attempts
                .iter()
                .find(|a| a.step == TransactionStep::Burn)
                .and_then(|a| a.tx_status.clone()))
        }
    }
    async fn replay(
        &self,
        _: &Operation,
    ) -> Result<(bool, Option<MintEvidence>, Option<u64>), String> {
        Ok((
            *self.consumed.lock().unwrap(),
            self.proof.lock().unwrap().clone(),
            Some(2000),
        ))
    }
    async fn receipt(&self, _: &Operation, _: &str) -> Result<Option<MintEvidence>, String> {
        Ok(self.proof.lock().unwrap().clone())
    }
    async fn prepare(&self, operation: &Operation) -> Result<PreparedCctpCall, String> {
        let data = operation.cctp.as_ref().unwrap();
        Ok(PreparedCctpCall {
            call: crate::tx::cctp::build_mint_call(
                &operation.cctp_transfer()?,
                &data.message,
                data.attestation.as_deref().unwrap(),
            )?,
            estimate: EvmCallEstimate {
                gas_limit: 200000,
                max_native_fee: 2000000,
                fees: EvmFeeEstimate {
                    max_fee_per_gas: 10,
                    max_priority_fee_per_gas: 1,
                },
            },
        })
    }
    async fn nonce(&self, _: &Operation) -> Result<u64, String> {
        Ok(self.nonce)
    }
    async fn preflight(&self, _: &Operation, _: &TransactionAttempt) -> Result<(), String> {
        if self.preflight_error {
            Err("RPC simulation failed".into())
        } else {
            Ok(())
        }
    }
    async fn broadcast(&self, operation: &Operation, _: &str) -> Result<String, String> {
        *self.broadcasts.lock().unwrap() += 1;
        if self.lost_response {
            Err("lost response".into())
        } else {
            Ok(operation.attempts.last().unwrap().tx_hash.clone())
        }
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
}
fn setup() -> (std::path::PathBuf, Mutex<AppState>, String) {
    let dir = temp_dir("mint");
    let state = execution_state(&dir);
    let result = create_source_operation(
        &state,
        source_asset(),
        destination_asset(),
        "0x4444444444444444444444444444444444444444".into(),
        "1000000".into(),
    )
    .unwrap();
    {
        let mut state = state.lock().unwrap();
        let operation = &mut state.wallet.as_mut().unwrap().operations[0];
        operation.step = OperationStep::AwaitingAttestation;
        let mut burn =
            crate::tests::sample_attempt(TransactionStep::Burn, TransactionStatus::Confirmed);
        burn.tx_status = Some(crate::cctp_attestation::tests::observation(
            Finality::Finalized,
        ));
        operation.attempts.push(burn);
        operation.cctp = Some(crate::cctp_attestation::tests::bound_data(operation));
    }
    (dir, state, result.operation_id)
}
fn proof(operation: &Operation, hash: &str) -> MintEvidence {
    MintEvidence {
        transaction_hash: hash.into(),
        message_hash: hex::encode(Keccak256::digest(&operation.cctp.as_ref().unwrap().message)),
        observation: TxStatus::new(Execution::Succeeded, Finality::Finalized),
    }
}

#[test]
fn consumed_flag_without_proof_never_completes_or_signs() {
    let (dir, state, id) = setup();
    let provider = MockMint::default();
    *provider.consumed.lock().unwrap() = true;
    runtime()
        .block_on(sync_cctp(&state, &provider, &id))
        .unwrap();
    assert!(
        runtime()
            .block_on(prepare_mint(&state, &provider, &id))
            .is_err()
    );
    let state = state.lock().unwrap();
    assert_eq!(
        state.wallet.as_ref().unwrap().operations[0].step,
        OperationStep::AwaitingAttestation
    );
    assert_eq!(
        state.wallet.as_ref().unwrap().operations[0].attempts.len(),
        1
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn failed_initial_save_prevents_mint_broadcast() {
    let (dir, state, id) = setup();
    std::fs::remove_file(dir.join("wallet.json")).unwrap();
    std::fs::create_dir(dir.join("wallet.json")).unwrap();
    let provider = MockMint::default();
    assert!(
        runtime()
            .block_on(prepare_mint(&state, &provider, &id))
            .is_err()
    );
    assert_eq!(*provider.broadcasts.lock().unwrap(), 0);
    assert_eq!(
        state.lock().unwrap().wallet.as_ref().unwrap().operations[0]
            .attempts
            .len(),
        1
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn finalized_external_mint_recovers_without_our_signed_transaction() {
    let (dir, state, id) = setup();
    let provider = MockMint::default();
    let hash = format!("0x{}", "77".repeat(32));
    *provider.consumed.lock().unwrap() = true;
    *provider.proof.lock().unwrap() = Some(proof(
        &state.lock().unwrap().wallet.as_ref().unwrap().operations[0],
        &hash,
    ));
    runtime()
        .block_on(sync_cctp(&state, &provider, &id))
        .unwrap();
    let state = state.lock().unwrap();
    let operation = &state.wallet.as_ref().unwrap().operations[0];
    assert_eq!(operation.step, OperationStep::Completed);
    assert!(operation.attempts.last().unwrap().raw_tx.is_none());
    assert_eq!(*provider.broadcasts.lock().unwrap(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn full_route_restarts_after_burn_and_polls_bound_attestation_before_mint() {
    use crate::cctp_execution::{
        prepare_source_step, submit_source_step,
        tests::{MockSourceProvider, approval_preflight, burn_preflight},
    };
    let dir = temp_dir("cctp-full-route");
    let state = execution_state(&dir);
    {
        let (key, salt) = crate::storage::derive_storage_key("TestPassword123!", None).unwrap();
        let mut state = state.lock().unwrap();
        state.encryption_key = Some(key);
        state.storage_salt = Some(salt);
    }
    let id = create_source_operation(
        &state,
        source_asset(),
        destination_asset(),
        "0x4444444444444444444444444444444444444444".into(),
        "1000000".into(),
    )
    .unwrap()
    .operation_id;
    let source = MockSourceProvider::default();
    let operation = state.lock().unwrap().wallet.as_ref().unwrap().operations[0].clone();
    *source.preflight.lock().unwrap() =
        Some(Ok(approval_preflight(&operation.cctp_transfer().unwrap())));
    let approval = runtime()
        .block_on(prepare_source_step(&state, &source, &id))
        .unwrap();
    source
        .broadcasts
        .lock()
        .unwrap()
        .push_back(Ok(approval.tx_hash.clone()));
    runtime()
        .block_on(submit_source_step(&state, &source, &id, &approval.tx_hash))
        .unwrap();
    *source.observation.lock().unwrap() = Some(Ok(Some(
        crate::cctp_attestation::tests::observation(Finality::Finalized),
    )));
    *source.pending_nonce.lock().unwrap() = 1;
    *source.preflight.lock().unwrap() =
        Some(Ok(burn_preflight(&operation.cctp_transfer().unwrap())));
    let burn = runtime()
        .block_on(prepare_source_step(&state, &source, &id))
        .unwrap();
    *source.observation.lock().unwrap() = Some(Ok(None));
    source
        .broadcasts
        .lock()
        .unwrap()
        .push_back(Ok(burn.tx_hash.clone()));
    runtime()
        .block_on(submit_source_step(&state, &source, &id, &burn.tx_hash))
        .unwrap();
    let stored = crate::storage::read_stored_wallet(&dir.join("wallet.json"))
        .unwrap()
        .unwrap();
    let (wallet, key, salt) = crate::storage::decrypt_wallet(&stored, "TestPassword123!")
        .unwrap()
        .into_parts();
    let restarted = Mutex::new(AppState::from_storage(dir.join("wallet.json")));
    {
        let mut state = restarted.lock().unwrap();
        state.wallet = Some(wallet);
        state.encryption_key = Some(key);
        state.storage_salt = Some(salt);
        state.locked = false;
        state.wallet_generation = 1;
        let operations = state.wallet.as_ref().unwrap().operations.clone();
        state.evm_nonces.restore_operations(&operations).unwrap();
    }
    let operation = restarted
        .lock()
        .unwrap()
        .wallet
        .as_ref()
        .unwrap()
        .operations[0]
        .clone();
    let attestation = FlowAttestation {
        operation,
        hash: burn.tx_hash.clone(),
    };
    let progress = runtime()
        .block_on(crate::cctp_attestation::poll_attestation(
            &restarted,
            &attestation,
            &id,
        ))
        .unwrap();
    assert_eq!(progress.state, "available");
    let mint = MockMint::default();
    let prepared = runtime()
        .block_on(prepare_mint(&restarted, &mint, &id))
        .unwrap();
    runtime()
        .block_on(submit_mint(&restarted, &mint, &id, &prepared.tx_hash))
        .unwrap();
    let operation = restarted
        .lock()
        .unwrap()
        .wallet
        .as_ref()
        .unwrap()
        .operations[0]
        .clone();
    *mint.proof.lock().unwrap() = Some(proof(&operation, &prepared.tx_hash));
    runtime()
        .block_on(sync_cctp(&restarted, &mint, &id))
        .unwrap();
    let state = restarted.lock().unwrap();
    let operation = &state.wallet.as_ref().unwrap().operations[0];
    assert_eq!(operation.step, OperationStep::Completed);
    assert_eq!(
        operation
            .attempts
            .iter()
            .filter(|a| a.step == TransactionStep::Burn)
            .count(),
        1
    );
    assert_eq!(
        operation
            .attempts
            .iter()
            .filter(|a| a.step == TransactionStep::Mint)
            .count(),
        1
    );
    assert_eq!(*source.broadcast_calls.lock().unwrap(), 2);
    assert_eq!(*mint.broadcasts.lock().unwrap(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn lost_response_and_restart_reuse_exact_mint_without_reburn() {
    let (dir, state, id) = setup();
    {
        let (key, salt) = crate::storage::derive_storage_key("TestPassword123!", None).unwrap();
        let mut state = state.lock().unwrap();
        state.encryption_key = Some(key);
        state.storage_salt = Some(salt);
    }
    let provider = MockMint {
        lost_response: true,
        ..MockMint::default()
    };
    let prepared = runtime()
        .block_on(prepare_mint(&state, &provider, &id))
        .unwrap();
    assert!(
        runtime()
            .block_on(submit_mint(&state, &provider, &id, &prepared.tx_hash))
            .is_err()
    );
    let stored = crate::storage::read_stored_wallet(&dir.join("wallet.json"))
        .unwrap()
        .unwrap();
    let restored = crate::storage::decrypt_wallet(&stored, "TestPassword123!").unwrap();
    {
        let mut state = state.lock().unwrap();
        let (wallet, key, salt) = restored.into_parts();
        state.wallet = Some(wallet);
        state.encryption_key = Some(key);
        state.storage_salt = Some(salt);
        state.evm_nonces = Default::default();
        let ops = state.wallet.as_ref().unwrap().operations.clone();
        state.evm_nonces.restore_operations(&ops).unwrap();
    }
    let again = runtime()
        .block_on(prepare_mint(&state, &provider, &id))
        .unwrap();
    assert_eq!(again.tx_hash, prepared.tx_hash);
    let operation = state.lock().unwrap().wallet.as_ref().unwrap().operations[0].clone();
    assert_eq!(
        operation
            .attempts
            .iter()
            .filter(|a| a.step == TransactionStep::Burn)
            .count(),
        1
    );
    assert_eq!(
        operation
            .attempts
            .iter()
            .filter(|a| a.step == TransactionStep::Mint)
            .count(),
        1
    );
    *provider.proof.lock().unwrap() = Some(proof(&operation, &prepared.tx_hash));
    runtime()
        .block_on(sync_cctp(&state, &provider, &id))
        .unwrap();
    assert_eq!(
        state.lock().unwrap().wallet.as_ref().unwrap().operations[0].step,
        OperationStep::Completed
    );
    assert_eq!(*provider.broadcasts.lock().unwrap(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn mismatched_finalized_message_proof_cannot_complete() {
    let (dir, state, id) = setup();
    let provider = MockMint::default();
    let mut invalid = proof(
        &state.lock().unwrap().wallet.as_ref().unwrap().operations[0],
        "external",
    );
    invalid.message_hash = "wrong".into();
    *provider.proof.lock().unwrap() = Some(invalid);
    assert!(
        runtime()
            .block_on(sync_cctp(&state, &provider, &id))
            .is_err()
    );
    assert_eq!(
        state.lock().unwrap().wallet.as_ref().unwrap().operations[0].step,
        OperationStep::AwaitingAttestation
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn preflight_failure_and_stale_review_never_broadcast() {
    let (dir, state, id) = setup();
    let provider = MockMint {
        preflight_error: true,
        ..MockMint::default()
    };
    let prepared = runtime()
        .block_on(prepare_mint(&state, &provider, &id))
        .unwrap();
    assert!(
        runtime()
            .block_on(submit_mint(&state, &provider, &id, "different hash"))
            .is_err()
    );
    assert!(
        runtime()
            .block_on(submit_mint(&state, &provider, &id, &prepared.tx_hash))
            .is_err()
    );
    assert_eq!(*provider.broadcasts.lock().unwrap(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

struct FlowAttestation {
    operation: Operation,
    hash: String,
}
impl crate::cctp_attestation::AttestationProvider for FlowAttestation {
    async fn receipt(
        &self,
        _: &crate::cctp::CctpTransfer,
        _: &str,
    ) -> Result<Option<(serde_json::Value, TxStatus)>, String> {
        let mut receipt = crate::cctp_attestation::tests::receipt(&self.operation);
        receipt["transactionHash"] = self.hash.clone().into();
        Ok(Some((
            receipt,
            crate::cctp_attestation::tests::observation(Finality::Finalized),
        )))
    }
    async fn messages(
        &self,
        _: &crate::cctp::CctpTransfer,
        _: &str,
    ) -> Result<crate::cctp_attestation::MessageResponse, String> {
        let mut response = crate::cctp_attestation::tests::response(&self.operation);
        response["sourceTxHash"] = self.hash.clone().into();
        Ok(crate::cctp_attestation::MessageResponse::Messages(response))
    }
    async fn verify(
        &self,
        _: &crate::cctp::CctpTransfer,
        _: &[u8],
        _: &[u8],
    ) -> Result<crate::cctp_attestation::AttestationCheck, String> {
        Ok(crate::cctp_attestation::AttestationCheck::DestinationSimulation)
    }
}

#[test]
fn refresh_rejects_uncertain_mints_and_changed_nonce_without_losing_saved_pair() {
    let (dir, state, id) = setup();
    let provider = MockMint::default();
    let prepared = runtime()
        .block_on(prepare_mint(&state, &provider, &id))
        .unwrap();
    let original = state.lock().unwrap().wallet.as_ref().unwrap().operations[0].clone();
    let mut changed = original.clone();
    changed.cctp.as_mut().unwrap().message[12] ^= 1;
    struct ChangedNonce(FlowAttestation);
    impl crate::cctp_attestation::AttestationProvider for ChangedNonce {
        async fn receipt(
            &self,
            t: &crate::cctp::CctpTransfer,
            h: &str,
        ) -> Result<Option<(serde_json::Value, TxStatus)>, String> {
            crate::cctp_attestation::AttestationProvider::receipt(&self.0, t, h).await
        }
        async fn messages(
            &self,
            _: &crate::cctp::CctpTransfer,
            _: &str,
        ) -> Result<crate::cctp_attestation::MessageResponse, String> {
            let mut response = crate::cctp_attestation::tests::response(&self.0.operation);
            response["messages"][0]["message"] = format!(
                "0x{}",
                hex::encode(&self.0.operation.cctp.as_ref().unwrap().message)
            )
            .into();
            Ok(crate::cctp_attestation::MessageResponse::Messages(response))
        }
        async fn verify(
            &self,
            _: &crate::cctp::CctpTransfer,
            _: &[u8],
            _: &[u8],
        ) -> Result<crate::cctp_attestation::AttestationCheck, String> {
            Ok(crate::cctp_attestation::AttestationCheck::DestinationSimulation)
        }
    }
    let attester = ChangedNonce(FlowAttestation {
        operation: changed,
        hash: original.attempts[0].tx_hash.clone(),
    });
    assert!(
        runtime()
            .block_on(crate::cctp_attestation::refresh_attestation(
                &state, &attester, &id
            ))
            .err()
            .unwrap()
            .contains("pinned mint nonce")
    );
    assert_eq!(
        state.lock().unwrap().wallet.as_ref().unwrap().operations[0],
        original
    );
    runtime()
        .block_on(submit_mint(&state, &provider, &id, &prepared.tx_hash))
        .unwrap();
    let before = state.lock().unwrap().wallet.as_ref().unwrap().operations[0].clone();
    assert!(
        runtime()
            .block_on(crate::cctp_attestation::refresh_attestation(
                &state, &attester, &id
            ))
            .err()
            .unwrap()
            .contains("pending or uncertain")
    );
    assert_eq!(
        state.lock().unwrap().wallet.as_ref().unwrap().operations[0],
        before
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn refresh_retires_only_unsubmitted_mint_and_reviews_new_fees_without_reburn() {
    let (dir, state, id) = setup();
    let provider = MockMint::default();
    let first = runtime()
        .block_on(prepare_mint(&state, &provider, &id))
        .unwrap();
    let operation = state.lock().unwrap().wallet.as_ref().unwrap().operations[0].clone();
    let attester = FlowAttestation {
        hash: operation.attempts[0].tx_hash.clone(),
        operation,
    };
    runtime()
        .block_on(crate::cctp_attestation::refresh_attestation(
            &state, &attester, &id,
        ))
        .unwrap();
    let second = runtime()
        .block_on(prepare_mint(&state, &provider, &id))
        .unwrap();
    let state = state.lock().unwrap();
    let operation = &state.wallet.as_ref().unwrap().operations[0];
    assert_eq!(
        operation
            .attempts
            .iter()
            .filter(|a| a.step == TransactionStep::Burn)
            .count(),
        1
    );
    assert_eq!(
        operation
            .attempts
            .iter()
            .filter(|a| a.step == TransactionStep::Mint)
            .count(),
        2
    );
    assert_eq!(operation.attempts[1].status, TransactionStatus::Superseded);
    assert_eq!(first.tx_hash, second.tx_hash);
    assert_eq!(*provider.broadcasts.lock().unwrap(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn source_finality_is_required_even_with_external_mint_proof() {
    let (dir, state, id) = setup();
    {
        let mut state = state.lock().unwrap();
        state.wallet.as_mut().unwrap().operations[0].attempts[0]
            .tx_status
            .as_mut()
            .unwrap()
            .finality = Finality::Included;
    }
    let provider = MockMint::default();
    *provider.proof.lock().unwrap() = Some(proof(
        &state.lock().unwrap().wallet.as_ref().unwrap().operations[0],
        "external",
    ));
    runtime()
        .block_on(sync_cctp(&state, &provider, &id))
        .unwrap();
    assert!(
        runtime()
            .block_on(prepare_mint(&state, &provider, &id))
            .is_err()
    );
    assert_eq!(*provider.broadcasts.lock().unwrap(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}
