use std::collections::HashMap;
use std::sync::Mutex;

use sha3::{Digest, Keccak256};
use solana_signer::Signer;

use super::{
    NativeProvider, prepare_native, signing_keys, submit_native, sync_native,
    validate_native_attempt,
};
use crate::cctp_execution::{
    create_source_operation,
    tests::{execution_state, source_asset, temp_dir},
};
use crate::dto::AssetIdentity;
use crate::operations::{
    MintEvidence, Operation, OperationStep, SolanaAttemptContext, TransactionAttempt,
    TransactionStatus, TransactionStep,
};
use crate::providers::cctp_native::NativePreflight;
use crate::state::AppState;
use crate::tx::cctp_solana::{
    burn_instruction, compute_budget, key, mint_instruction, setup_instructions,
};
use crate::tx::tx_status::{Execution, Finality, TxStatus};

#[derive(Default)]
struct MockNative {
    observations: Mutex<HashMap<String, TxStatus>>,
    proof: Mutex<Option<MintEvidence>>,
    raws: Mutex<Vec<String>>,
    counter: Mutex<u8>,
    lost: bool,
    simulation_error: bool,
    expired: bool,
    consumed: bool,
}
impl NativeProvider for MockNative {
    async fn observe(
        &self,
        _: &Operation,
        _: bool,
        hash: &str,
    ) -> Result<Option<TxStatus>, String> {
        Ok(self.observations.lock().unwrap().get(hash).cloned())
    }
    async fn prepare(
        &self,
        operation: &Operation,
        event: Option<&str>,
        setup: bool,
    ) -> Result<NativePreflight, String> {
        let transfer = operation.cctp_transfer()?;
        let destination = setup
            || operation.step == OperationStep::Mint
            || crate::operations::attestation_available(operation);
        let payer = if destination {
            transfer.destination_fee_payer()?
        } else {
            transfer.sender()
        };
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        let mut context = SolanaAttemptContext {
            payer: payer.into(),
            blockhash: bs58::encode([*counter; 32]).into_string(),
            last_valid_height: 500,
            fee_lamports: 5000,
            rent_lamports: 1000000,
            event_account: None,
            fee_recipient: None,
            lookup: None,
            setup_slot: None,
            create_ata: false,
            provisional_destination_fee: None,
        };
        let mut tables = Vec::new();
        let (step, instructions) = if setup {
            let fee = key(payer)?;
            let (instructions, lookup) = setup_instructions(&transfer, fee, fee, 40, true)?;
            context.lookup = Some(lookup);
            context.fee_recipient = Some(payer.into());
            context.setup_slot = Some(40);
            context.create_ata = true;
            (TransactionStep::DestinationSetup, instructions)
        } else if destination {
            let data = operation.cctp.as_ref().unwrap();
            let lookup = data.native_lookup.as_ref().unwrap();
            context.lookup = Some(lookup.clone());
            context.fee_recipient = Some(payer.into());
            tables.push(solana_message::AddressLookupTableAccount {
                key: key(&lookup.address)?,
                addresses: lookup
                    .addresses
                    .iter()
                    .map(|a| key(a))
                    .collect::<Result<Vec<_>, _>>()?,
            });
            (
                TransactionStep::Mint,
                vec![
                    compute_budget()?,
                    mint_instruction(
                        &transfer,
                        key(payer)?,
                        key(payer)?,
                        &data.message,
                        data.attestation.as_deref().unwrap(),
                    )?,
                ],
            )
        } else {
            context.event_account = event.map(str::to_string);
            (
                TransactionStep::Burn,
                vec![
                    compute_budget()?,
                    burn_instruction(&transfer, key(event.unwrap())?)?,
                ],
            )
        };
        Ok(NativePreflight {
            step,
            instructions,
            context,
            tables,
        })
    }
    async fn expired(&self, _: &Operation, _: &TransactionAttempt) -> Result<bool, String> {
        Ok(self.expired)
    }
    async fn receipt(&self, _: &Operation, _: &str) -> Result<Option<MintEvidence>, String> {
        Ok(self.proof.lock().unwrap().clone())
    }
    async fn replay(
        &self,
        _: &Operation,
    ) -> Result<(bool, Option<MintEvidence>, Option<String>), String> {
        Ok((self.consumed, self.proof.lock().unwrap().clone(), None))
    }
    async fn simulate(&self, _: &Operation, _: TransactionStep, _: &str) -> Result<(), String> {
        if self.simulation_error {
            Err("simulation failed".into())
        } else {
            Ok(())
        }
    }
    async fn preflight_signed(
        &self,
        operation: &Operation,
        attempt: &TransactionAttempt,
    ) -> Result<(), String> {
        validate_native_attempt(operation, attempt)
    }
    async fn broadcast(
        &self,
        operation: &Operation,
        _: TransactionStep,
        raw: &str,
    ) -> Result<String, String> {
        self.raws.lock().unwrap().push(raw.into());
        let attempt = operation.attempts.last().unwrap();
        assert_eq!(attempt.status, TransactionStatus::SubmissionUnknown);
        if self.lost {
            Err("lost response".into())
        } else {
            Ok(attempt.tx_hash.clone())
        }
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
}
fn setup(source_solana: bool) -> (std::path::PathBuf, Mutex<AppState>, String) {
    let dir = temp_dir("cctp-native");
    let state = execution_state(&dir);
    let address = {
        let mut state = state.lock().unwrap();
        let wallet = state.wallet.as_mut().unwrap();
        let (payer, _) = signing_keys(wallet, "fixture").unwrap();
        let address = payer.pubkey().to_string();
        wallet.addresses.insert("solana".into(), address.clone());
        address
    };
    let sol = AssetIdentity::new(
        "solana",
        Some("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
    );
    let (source, destination, recipient) = if source_solana {
        (
            sol,
            source_asset(),
            "0x4444444444444444444444444444444444444444".into(),
        )
    } else {
        (source_asset(), sol, address)
    };
    let id = create_source_operation(&state, source, destination, recipient, "1000000".into())
        .unwrap()
        .operation_id;
    (dir, state, id)
}
fn prepare_destination_mint(state: &Mutex<AppState>, provider: &MockNative, id: &str) {
    let setup = runtime()
        .block_on(prepare_native(state, provider, id, true))
        .unwrap();
    runtime()
        .block_on(submit_native(state, provider, id, &setup.tx_hash))
        .unwrap();
    let mut observation = TxStatus::new(Execution::Succeeded, Finality::Finalized);
    observation.slot = Some(42);
    provider
        .observations
        .lock()
        .unwrap()
        .insert(setup.tx_hash, observation);
    runtime()
        .block_on(sync_native(state, provider, id))
        .unwrap();
    let mut state = state.lock().unwrap();
    let operation = &mut state.wallet.as_mut().unwrap().operations[0];
    operation.step = OperationStep::AwaitingAttestation;
    let mut burn =
        crate::tests::sample_attempt(TransactionStep::Burn, TransactionStatus::Confirmed);
    burn.tx_status = Some(crate::cctp_attestation::tests::observation(
        Finality::Finalized,
    ));
    operation.attempts.push(burn);
    let lookup = operation.cctp.as_ref().unwrap().native_lookup.clone();
    let mut data = crate::cctp_attestation::tests::bound_data(operation);
    data.simulated = false;
    data.signature_verified = true;
    data.native_lookup = lookup;
    operation.cctp = Some(data);
}

#[test]
fn burn_lost_response_keeps_signed_bytes_and_never_signs_again() {
    let (dir, state, id) = setup(true);
    let provider = MockNative {
        lost: true,
        ..Default::default()
    };
    let first = runtime()
        .block_on(prepare_native(&state, &provider, &id, false))
        .unwrap();
    assert!(
        runtime()
            .block_on(submit_native(&state, &provider, &id, &first.tx_hash))
            .is_err()
    );
    let second = runtime()
        .block_on(prepare_native(&state, &provider, &id, false))
        .unwrap();
    assert_eq!(first.tx_hash, second.tx_hash);
    assert_eq!(
        state.lock().unwrap().wallet.as_ref().unwrap().operations[0]
            .attempts
            .len(),
        1
    );
    assert_eq!(provider.raws.lock().unwrap().len(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn consumed_native_nonce_without_proof_never_completes() {
    let (dir, state, id) = setup(false);
    let provider = MockNative::default();
    prepare_destination_mint(&state, &provider, &id);
    let provider = MockNative {
        consumed: true,
        ..Default::default()
    };
    runtime()
        .block_on(sync_native(&state, &provider, &id))
        .unwrap();
    assert!(
        runtime()
            .block_on(prepare_native(&state, &provider, &id, false))
            .is_err()
    );
    assert_eq!(
        state.lock().unwrap().wallet.as_ref().unwrap().operations[0].step,
        OperationStep::AwaitingAttestation
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn expired_mint_renews_blockhash_and_keeps_same_bound_pair() {
    let (dir, state, id) = setup(false);
    let provider = MockNative::default();
    prepare_destination_mint(&state, &provider, &id);
    let first = runtime()
        .block_on(prepare_native(&state, &provider, &id, false))
        .unwrap();
    let provider = MockNative {
        expired: true,
        ..Default::default()
    };
    *provider.counter.lock().unwrap() = 10;
    let second = runtime()
        .block_on(prepare_native(&state, &provider, &id, false))
        .unwrap();
    assert_ne!(first.tx_hash, second.tx_hash);
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
    assert_eq!(
        operation.cctp.as_ref().unwrap().message,
        crate::cctp_attestation::tests::attested(operation)
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn finalized_native_mint_completes_after_setup_and_bound_attestation() {
    let (dir, state, id) = setup(false);
    let provider = MockNative::default();
    prepare_destination_mint(&state, &provider, &id);
    let prepared = runtime()
        .block_on(prepare_native(&state, &provider, &id, false))
        .unwrap();
    runtime()
        .block_on(submit_native(&state, &provider, &id, &prepared.tx_hash))
        .unwrap();
    let operation = state.lock().unwrap().wallet.as_ref().unwrap().operations[0].clone();
    let mut observation = TxStatus::new(Execution::Succeeded, Finality::Finalized);
    observation.slot = Some(55);
    *provider.proof.lock().unwrap() = Some(MintEvidence {
        transaction_hash: prepared.tx_hash.clone(),
        message_hash: hex::encode(Keccak256::digest(&operation.cctp.as_ref().unwrap().message)),
        observation,
    });
    runtime()
        .block_on(sync_native(&state, &provider, &id))
        .unwrap();
    assert_eq!(
        state.lock().unwrap().wallet.as_ref().unwrap().operations[0].step,
        OperationStep::Completed
    );
    assert_eq!(provider.raws.lock().unwrap().len(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn native_save_or_simulation_failure_prevents_broadcast() {
    for save in [false, true] {
        let (dir, state, id) = setup(true);
        if save {
            std::fs::remove_file(dir.join("wallet.json")).unwrap();
            std::fs::create_dir(dir.join("wallet.json")).unwrap();
        }
        let provider = MockNative {
            simulation_error: !save,
            ..Default::default()
        };
        assert!(
            runtime()
                .block_on(prepare_native(&state, &provider, &id, false))
                .is_err()
        );
        assert!(provider.raws.lock().unwrap().is_empty());
        assert!(
            state.lock().unwrap().wallet.as_ref().unwrap().operations[0]
                .attempts
                .is_empty()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
#[test]
fn tampered_native_intent_and_signature_are_rejected() {
    let (dir, state, id) = setup(true);
    let provider = MockNative::default();
    runtime()
        .block_on(prepare_native(&state, &provider, &id, false))
        .unwrap();
    let operation = state.lock().unwrap().wallet.as_ref().unwrap().operations[0].clone();
    let attempt = operation.attempts[0].clone();
    validate_native_attempt(&operation, &attempt).unwrap();
    let mut changed = operation.clone();
    changed.amount_base_units = "2000000".into();
    assert!(validate_native_attempt(&changed, &attempt).is_err());
    let mut changed = attempt.clone();
    changed.tx_hash = bs58::encode([0; 64]).into_string();
    assert!(validate_native_attempt(&operation, &changed).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}
