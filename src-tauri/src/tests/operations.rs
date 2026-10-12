use super::{
    CctpData, Operation, OperationStep, TransactionStatus, TransactionStep, advance_operation,
    cancel_operation, create_operation, operation_activity, reconcile_operation,
    reconcile_operation_with, record_attestation, record_operation_error,
    record_transaction_attempt,
};
use crate::providers::http::ProviderClients;
use crate::tests::{sample_attempt, sample_operation};
use crate::tx::tx_status::{Execution, Finality, TxStatus};

#[test]
fn advances_only_with_required_evidence() {
    let mut operation = sample_operation();
    record_transaction_attempt(
        &mut operation,
        TransactionStep::Approval,
        sample_attempt(TransactionStep::Approval, TransactionStatus::Submitted),
    )
    .unwrap();
    let original = operation.clone();
    assert!(advance_operation(&mut operation, OperationStep::Burn).is_err());
    assert_eq!(operation, original);
    operation.attempts[0].status = TransactionStatus::Confirmed;
    advance_operation(&mut operation, OperationStep::Burn).unwrap();
    let original = operation.clone();
    assert!(advance_operation(&mut operation, OperationStep::AwaitingAttestation).is_err());
    assert_eq!(operation, original);
    record_transaction_attempt(
        &mut operation,
        TransactionStep::Burn,
        sample_attempt(TransactionStep::Burn, TransactionStatus::SubmissionUnknown),
    )
    .unwrap();
    advance_operation(&mut operation, OperationStep::AwaitingAttestation).unwrap();
    assert!(advance_operation(&mut operation, OperationStep::Mint).is_err());
    operation.cctp = Some(CctpData {
        message: vec![1, 2],
        attestation: Some(vec![3, 4]),
    });
    assert!(advance_operation(&mut operation, OperationStep::Mint).is_err());
    operation.attempts.last_mut().unwrap().tx_status =
        Some(TxStatus::new(Execution::Succeeded, Finality::Finalized));
    advance_operation(&mut operation, OperationStep::Mint).unwrap();
    assert!(advance_operation(&mut operation, OperationStep::Completed).is_err());
    record_transaction_attempt(
        &mut operation,
        TransactionStep::Mint,
        sample_attempt(TransactionStep::Mint, TransactionStatus::Confirmed),
    )
    .unwrap();
    operation.attempts.last_mut().unwrap().tx_status =
        Some(TxStatus::new(Execution::Succeeded, Finality::Finalized));
    advance_operation(&mut operation, OperationStep::Completed).unwrap();
    let original = operation.clone();
    assert!(advance_operation(&mut operation, OperationStep::Burn).is_err());
    assert_eq!(operation, original);
}

#[test]
fn attestation_storage_checks_stage_and_bounds_without_advancing() {
    let mut operation = sample_operation();
    let original = operation.clone();
    assert!(record_attestation(&mut operation, vec![1], vec![2]).is_err());
    assert_eq!(operation, original);
    operation.step = OperationStep::AwaitingAttestation;
    for (message, attestation) in [
        (vec![], vec![2]),
        (vec![1], vec![]),
        (vec![1; 16_385], vec![2]),
        (vec![1], vec![2; 16_385]),
    ] {
        let original = operation.clone();
        assert!(record_attestation(&mut operation, message, attestation).is_err());
        assert_eq!(operation, original);
    }
    record_attestation(&mut operation, vec![1], vec![2]).unwrap();
    assert_eq!(operation.step, OperationStep::AwaitingAttestation);
    assert_eq!(operation.cctp.unwrap().attestation, Some(vec![2]));
}

#[test]
fn cancellation_rejects_signed_submitted_uncertain_and_confirmed_burns() {
    for status in [
        TransactionStatus::Signed,
        TransactionStatus::Submitted,
        TransactionStatus::SubmissionUnknown,
        TransactionStatus::Confirmed,
    ] {
        let mut operation = sample_operation();
        operation
            .attempts
            .push(sample_attempt(TransactionStep::Burn, status));
        let original = operation.clone();
        assert!(cancel_operation(&mut operation).is_err());
        assert_eq!(operation, original);
    }
    let mut operation = sample_operation();
    operation.step = OperationStep::Burn;
    let original = operation.clone();
    assert!(cancel_operation(&mut operation).is_err());
    assert_eq!(operation, original);
    operation.step = OperationStep::Approval;
    cancel_operation(&mut operation).unwrap();
    assert_eq!(operation.step, OperationStep::Cancelled);
}

#[test]
fn creates_normalized_identity_and_rejects_invalid_requests() {
    let operation = sample_operation();
    assert_eq!(operation.amount_base_units, "1000000");
    assert_eq!(operation.step, OperationStep::Approval);
    assert!(operation.attempts.is_empty());
    assert!(operation.cctp.is_none());
    assert_eq!(operation.required_source_finality, Finality::Finalized);
    assert!(
        create_operation(
            operation.source_asset.clone(),
            crate::dto::AssetIdentity::new(
                "base",
                Some("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913")
            ),
            operation.sender.clone(),
            operation.recipient.clone(),
            1,
        )
        .is_err()
    );
    for amount in [0, 1] {
        assert!(
            create_operation(
                operation.source_asset.clone(),
                operation.source_asset.clone(),
                operation.sender.clone(),
                operation.recipient.clone(),
                amount
            )
            .is_err()
        );
    }
    assert!(
        create_operation(
            operation.source_asset.clone(),
            operation.destination_asset.clone(),
            "invalid".to_string(),
            operation.recipient.clone(),
            1
        )
        .is_err()
    );
    assert!(
        create_operation(
            operation.source_asset.clone(),
            operation.destination_asset.clone(),
            operation.sender,
            operation.recipient,
            0
        )
        .is_err()
    );
}

#[test]
fn errors_preserve_recovery_position_and_transaction_history() {
    let mut operation = sample_operation();
    operation.step = OperationStep::AwaitingAttestation;
    operation.attempts.push(sample_attempt(
        TransactionStep::Burn,
        TransactionStatus::Confirmed,
    ));
    operation.cctp = Some(CctpData {
        message: vec![1],
        attestation: None,
    });
    let original = operation.clone();
    record_operation_error(&mut operation, " ".to_string());
    assert_eq!(operation, original);
    record_operation_error(&mut operation, "attestation unavailable".to_string());
    assert_eq!(operation.step, original.step);
    assert_eq!(operation.attempts, original.attempts);
    assert_eq!(operation.cctp, original.cctp);
    assert_eq!(
        operation.last_error.as_deref(),
        Some("attestation unavailable")
    );
}

#[test]
fn included_observations_remain_trackable_and_do_not_complete_mint() {
    let mut operation = sample_operation();
    operation.step = OperationStep::Mint;
    operation.attempts.push(sample_attempt(
        TransactionStep::Mint,
        TransactionStatus::Signed,
    ));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let included = TxStatus::new(Execution::Succeeded, Finality::Included);
    runtime
        .block_on(reconcile_operation_with(&mut operation, |_, _| {
            std::future::ready(Ok(Some(included.clone())))
        }))
        .unwrap();
    assert_eq!(operation.attempts[0].status, TransactionStatus::Submitted);
    assert_eq!(operation.attempts[0].tx_status, Some(included.clone()));
    assert!(advance_operation(&mut operation, OperationStep::Completed).is_err());
    let unchanged = operation.clone();
    runtime
        .block_on(reconcile_operation_with(&mut operation, |_, _| {
            std::future::ready(Ok(Some(included.clone())))
        }))
        .unwrap();
    assert_eq!(operation, unchanged);
    runtime
        .block_on(reconcile_operation_with(&mut operation, |_, _| {
            std::future::ready(Ok(Some(TxStatus::new(
                Execution::Succeeded,
                Finality::Finalized,
            ))))
        }))
        .unwrap();
    let mut burn = sample_attempt(TransactionStep::Burn, TransactionStatus::Confirmed);
    burn.tx_status = Some(TxStatus::new(Execution::Succeeded, Finality::Finalized));
    operation.attempts.push(burn);
    advance_operation(&mut operation, OperationStep::Completed).unwrap();
}

#[test]
fn legacy_confirmed_mints_do_not_imply_finality() {
    let mut operation = sample_operation();
    operation.step = OperationStep::Mint;
    operation.attempts.push(sample_attempt(
        TransactionStep::Mint,
        TransactionStatus::Confirmed,
    ));
    assert!(advance_operation(&mut operation, OperationStep::Completed).is_err());
}

#[test]
fn mint_requires_successful_source_evidence_meeting_the_recorded_policy() {
    let mut operation = sample_operation();
    operation.step = OperationStep::AwaitingAttestation;
    operation.cctp = Some(CctpData {
        message: vec![1],
        attestation: Some(vec![2]),
    });
    let mut burn = sample_attempt(TransactionStep::Burn, TransactionStatus::Submitted);
    burn.tx_status = Some(TxStatus::new(Execution::Succeeded, Finality::Included));
    operation.attempts.push(burn);
    let before = operation.clone();
    assert!(advance_operation(&mut operation, OperationStep::Mint).is_err());
    assert_eq!(operation, before);
    operation.required_source_finality = Finality::Included;
    assert_eq!(
        operation_activity(&operation).unwrap().title,
        "Attestation available"
    );
    advance_operation(&mut operation, OperationStep::Mint).unwrap();
    operation.step = OperationStep::AwaitingAttestation;
    operation.required_source_finality = Finality::Finalized;
    assert_eq!(
        operation_activity(&operation).unwrap().title,
        "Waiting for source finality"
    );
    operation.attempts[0].tx_status = Some(TxStatus::new(Execution::Failed, Finality::Finalized));
    assert!(advance_operation(&mut operation, OperationStep::Mint).is_err());
    operation.attempts[0].tx_status =
        Some(TxStatus::new(Execution::Succeeded, Finality::Finalized));
    advance_operation(&mut operation, OperationStep::Mint).unwrap();
    let mut legacy = serde_json::to_value(sample_operation()).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("required_source_finality");
    let restored: Operation = serde_json::from_value(legacy).unwrap();
    assert_eq!(restored.required_source_finality, Finality::Finalized);
    let mut legacy_mint = sample_operation();
    legacy_mint.step = OperationStep::Mint;
    let mut minted = sample_attempt(TransactionStep::Mint, TransactionStatus::Confirmed);
    minted.tx_status = Some(TxStatus::new(Execution::Succeeded, Finality::Finalized));
    legacy_mint.attempts.push(minted);
    assert!(advance_operation(&mut legacy_mint, OperationStep::Completed).is_err());
}

#[test]
fn missing_operation_receipts_clear_inclusion_without_losing_recovery_data() {
    let mut operation = sample_operation();
    let mut attempt = sample_attempt(TransactionStep::Burn, TransactionStatus::Submitted);
    attempt.tx_status = Some(TxStatus::new(Execution::Succeeded, Finality::Included));
    operation.attempts.push(attempt.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime
        .block_on(reconcile_operation_with(&mut operation, |_, _| {
            std::future::ready(Err("RPC unavailable".to_string()))
        }))
        .unwrap();
    assert_eq!(operation.attempts[0].tx_status, attempt.tx_status);
    runtime
        .block_on(reconcile_operation_with(&mut operation, |_, _| {
            std::future::ready(Ok(None))
        }))
        .unwrap();
    assert!(operation.attempts[0].tx_status.is_none());
    assert_eq!(
        operation.attempts[0].status,
        TransactionStatus::SubmissionUnknown
    );
    assert_eq!(operation.attempts[0].tx_hash, attempt.tx_hash);
    assert_eq!(operation.attempts[0].raw_tx, attempt.raw_tx);
    assert_eq!(operation.step, OperationStep::Approval);
    assert!(cancel_operation(&mut operation).is_err());
}

#[test]
fn nonfinal_failed_burns_cannot_be_cancelled_and_are_rechecked() {
    let mut operation = sample_operation();
    let mut attempt = sample_attempt(TransactionStep::Burn, TransactionStatus::Failed);
    attempt.tx_status = Some(TxStatus::new(Execution::Failed, Finality::Included));
    operation.attempts.push(attempt);
    assert!(cancel_operation(&mut operation).is_err());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime
        .block_on(reconcile_operation_with(&mut operation, |_, _| {
            std::future::ready(Ok(Some(TxStatus::new(
                Execution::Failed,
                Finality::Finalized,
            ))))
        }))
        .unwrap();
    assert_eq!(
        operation.attempts[0].tx_status.as_ref().unwrap().finality,
        Finality::Finalized
    );
    cancel_operation(&mut operation).unwrap();
}

#[test]
fn operation_activity_uses_current_attempt_and_matching_network() {
    let mut operation = sample_operation();
    operation.step = OperationStep::Mint;
    operation.attempts.push(sample_attempt(
        TransactionStep::Burn,
        TransactionStatus::Failed,
    ));
    operation.attempts.push(sample_attempt(
        TransactionStep::Burn,
        TransactionStatus::Confirmed,
    ));
    operation.attempts.push(sample_attempt(
        TransactionStep::Mint,
        TransactionStatus::Submitted,
    ));
    let activity = operation_activity(&operation).unwrap();
    assert_eq!(activity.status, "pending");
    assert_eq!(activity.network.as_deref(), Some("arbitrum_one"));
    assert_eq!(activity.hash, operation.attempts[2].tx_hash);
    assert_eq!(activity.id, operation.id);
    let original = operation.clone();
    operation_activity(&operation).unwrap();
    assert_eq!(operation, original);
    operation.step = OperationStep::Completed;
    assert_eq!(operation_activity(&operation).unwrap().status, "confirmed");
}

#[test]
fn reconciliation_checks_signed_hashes_after_a_broadcast_crash() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    for step in [
        TransactionStep::Approval,
        TransactionStep::Burn,
        TransactionStep::Mint,
    ] {
        let mut operation = sample_operation();
        let attempt = sample_attempt(step, TransactionStatus::Signed);
        operation.attempts.push(attempt.clone());
        let mut calls = 0;
        runtime
            .block_on(reconcile_operation_with(&mut operation, |network, hash| {
                calls += 1;
                assert_eq!(hash, attempt.tx_hash);
                assert_eq!(
                    network,
                    if step == TransactionStep::Mint {
                        "arbitrum_one"
                    } else {
                        "ethereum"
                    }
                );
                std::future::ready(Ok(Some(TxStatus::new(
                    Execution::Succeeded,
                    Finality::Finalized,
                ))))
            }))
            .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(operation.attempts[0].status, TransactionStatus::Confirmed);
        assert_eq!(operation.attempts[0].raw_tx, attempt.raw_tx);
        assert_eq!(operation.step, OperationStep::Approval);
        assert_eq!(operation.attempts.len(), 1);
    }
}

#[test]
fn reconciliation_keeps_signed_attempts_when_receipt_is_absent_or_lookup_fails() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    for result in [Ok(None), Err("RPC unavailable".to_string())] {
        let mut operation = sample_operation();
        let attempt = sample_attempt(TransactionStep::Burn, TransactionStatus::Signed);
        operation.attempts.push(attempt.clone());
        let original_step = operation.step;
        runtime
            .block_on(reconcile_operation_with(&mut operation, |_, _| {
                std::future::ready(result.clone())
            }))
            .unwrap();
        assert_eq!(operation.attempts[0].status, TransactionStatus::Signed);
        assert_eq!(operation.attempts[0].tx_hash, attempt.tx_hash);
        assert_eq!(operation.attempts[0].raw_tx, attempt.raw_tx);
        assert_eq!(operation.step, original_step);
        assert_eq!(operation.attempts.len(), 1);
    }
}

#[test]
fn reconciliation_preserves_uncertainty_when_network_is_unavailable() {
    let mut operation = sample_operation();
    operation.deployment = None; // legacy unsupported records retain read-only diagnostics
    operation.source_asset.network = "unsupported".to_string();
    operation.attempts.push(sample_attempt(
        TransactionStep::Burn,
        TransactionStatus::SubmissionUnknown,
    ));
    let hash = operation.attempts[0].tx_hash.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime
        .block_on(reconcile_operation(
            &mut operation,
            &ProviderClients::new().unwrap(),
        ))
        .unwrap();
    assert_eq!(
        operation.attempts[0].status,
        TransactionStatus::SubmissionUnknown
    );
    assert_eq!(operation.attempts[0].tx_hash, hash);
    assert_eq!(operation.step, OperationStep::Approval);
    assert!(operation.last_error.is_some());
}

#[test]
fn rejects_backwards_skipped_and_same_step_transitions() {
    let mut operation = sample_operation();
    for next in [
        OperationStep::Approval,
        OperationStep::Mint,
        OperationStep::Completed,
    ] {
        let original = operation.clone();
        assert!(advance_operation(&mut operation, next).is_err());
        assert_eq!(operation, original);
    }
    advance_operation(&mut operation, OperationStep::Burn).unwrap();
    let original = operation.clone();
    assert!(advance_operation(&mut operation, OperationStep::Approval).is_err());
    assert_eq!(operation, original);
}

#[test]
fn retains_attempt_history_and_rejects_invalid_or_duplicate_confirmed_legs() {
    let mut operation = sample_operation();
    for status in [
        TransactionStatus::Signed,
        TransactionStatus::Submitted,
        TransactionStatus::Confirmed,
    ] {
        record_transaction_attempt(
            &mut operation,
            TransactionStep::Burn,
            sample_attempt(TransactionStep::Burn, status),
        )
        .unwrap();
    }
    assert_eq!(operation.attempts.len(), 3);
    let original = operation.clone();
    assert!(
        record_transaction_attempt(
            &mut operation,
            TransactionStep::Burn,
            sample_attempt(TransactionStep::Burn, TransactionStatus::Submitted)
        )
        .is_err()
    );
    assert_eq!(operation, original);
    assert!(
        record_transaction_attempt(
            &mut operation,
            TransactionStep::Mint,
            sample_attempt(TransactionStep::Burn, TransactionStatus::Submitted)
        )
        .is_err()
    );
    let mut invalid = sample_attempt(TransactionStep::Mint, TransactionStatus::Signed);
    invalid.tx_hash.clear();
    assert!(record_transaction_attempt(&mut operation, TransactionStep::Mint, invalid).is_err());
    assert_eq!(operation, original);
}

#[test]
fn round_trips_operations_with_and_without_cctp_payloads() {
    let mut operation = sample_operation();
    for cctp in [
        None,
        Some(CctpData {
            message: vec![1, 2],
            attestation: Some(vec![3, 4]),
        }),
    ] {
        operation.cctp = cctp;
        operation.attempts = vec![sample_attempt(
            TransactionStep::Burn,
            TransactionStatus::SubmissionUnknown,
        )];
        let restored: Operation =
            serde_json::from_slice(&serde_json::to_vec(&operation).unwrap()).unwrap();
        assert_eq!(restored, operation);
    }
}
