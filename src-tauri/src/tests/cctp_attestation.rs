use super::*;
use crate::operations::{TransactionStatus, TransactionStep};
use crate::tests::{sample_attempt, sample_operation};

pub(crate) fn emitted(operation: &Operation) -> Vec<u8> {
    let t = operation.cctp_transfer().unwrap();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&t.route.source.domain.to_be_bytes());
    bytes.extend_from_slice(&t.route.destination.domain.to_be_bytes());
    bytes.extend_from_slice(&[0; 32]);
    bytes.extend_from_slice(
        &address_word(&t.route.source.network, t.route.source.token_messenger).unwrap(),
    );
    bytes.extend_from_slice(
        &address_word(
            &t.route.destination.network,
            t.route.destination.token_messenger,
        )
        .unwrap(),
    );
    bytes.extend_from_slice(&t.caller_word().unwrap());
    bytes.extend_from_slice(&2000u32.to_be_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&address_word(&t.route.source.network, t.route.source.usdc).unwrap());
    bytes.extend_from_slice(&t.mint_recipient_word().unwrap());
    bytes.extend_from_slice(&evm_uint_word(t.amount()));
    bytes.extend_from_slice(&address_word(&t.route.source.network, t.sender()).unwrap());
    bytes.extend_from_slice(&[0; 96]);
    bytes
}

pub(crate) fn evidence(operation: &Operation) -> BurnEvidence {
    BurnEvidence {
        transaction_hash: format!("0x{}", "ab".repeat(32)),
        block_hash: Some(format!("0x{}", "cd".repeat(32))),
        block_height: Some(42),
        message_log_index: Some(1),
        deposit_log_index: Some(2),
        slot: None,
        message_account: None,
        emitted_message: emitted(operation),
    }
}

pub(crate) fn attested(operation: &Operation) -> Vec<u8> {
    let mut bytes = emitted(operation);
    bytes[12..44].fill(0xab);
    bytes[144..148].copy_from_slice(&2000u32.to_be_bytes());
    bytes
}

pub(crate) fn bound_data(operation: &Operation) -> CctpData {
    CctpData {
        message: attested(operation),
        attestation: Some(vec![1; 65]),
        evidence: Some(evidence(operation)),
        nonce: Some(format!("0x{}", "ab".repeat(32))),
        simulated: true,
        signature_verified: false,
        poll: None,
    }
}

pub(crate) fn observation(finality: Finality) -> TxStatus {
    let mut status = TxStatus::new(Execution::Succeeded, finality);
    status.block_height = Some(42);
    status.block_hash = Some(format!("0x{}", "cd".repeat(32)));
    status
}

pub(crate) fn operation() -> Operation {
    let mut operation = sample_operation();
    operation.step = OperationStep::AwaitingAttestation;
    let mut burn = sample_attempt(TransactionStep::Burn, TransactionStatus::Submitted);
    burn.tx_status = Some(observation(Finality::Included));
    operation.attempts.push(burn);
    operation
}

pub(crate) fn receipt(operation: &Operation) -> serde_json::Value {
    let t = operation.cctp_transfer().unwrap();
    let evidence = evidence(operation);
    let message = emitted(operation);
    let mut data = Vec::new();
    data.extend_from_slice(&evm_uint_word(32));
    data.extend_from_slice(&evm_uint_word(message.len() as u128));
    data.extend_from_slice(&message);
    data.resize(64 + message.len().div_ceil(32) * 32, 0);
    let mut deposit = Vec::new();
    for word in [
        evm_uint_word(t.amount()),
        evm_address_word(t.recipient()).unwrap(),
        evm_uint_word(3),
        evm_address_word(t.route.destination.token_messenger).unwrap(),
        evm_address_word(t.destination_caller()).unwrap(),
        evm_uint_word(0),
        evm_uint_word(224),
        evm_uint_word(0),
    ] {
        deposit.extend_from_slice(&word);
    }
    serde_json::json!({"status":"0x1","transactionHash":evidence.transaction_hash,"blockHash":evidence.block_hash,"blockNumber":"0x2a","logs":[
        {"address":t.route.source.message_transmitter,"topics":[format!("0x{}",hex::encode(event_topic("MessageSent(bytes)")))],"data":format!("0x{}",hex::encode(data)),"logIndex":"0x1","removed":false},
        {"address":t.route.source.token_messenger,"topics":[format!("0x{}",hex::encode(event_topic("DepositForBurn(address,uint256,address,bytes32,uint32,bytes32,bytes32,uint256,uint32,bytes)"))),format!("0x{}",hex::encode(evm_address_word(t.route.source.usdc).unwrap())),format!("0x{}",hex::encode(evm_address_word(t.sender()).unwrap())),format!("0x{}",hex::encode(evm_uint_word(2000)))],"data":format!("0x{}",hex::encode(deposit)),"logIndex":"0x2","removed":false}
    ]})
}

pub(crate) fn response(operation: &Operation) -> serde_json::Value {
    serde_json::json!({"sourceTxHash":evidence(operation).transaction_hash,"messages":[{"status":"complete","message":format!("0x{}",hex::encode(attested(operation))),"attestation":format!("0x{}","01".repeat(65))}]})
}

#[test]
fn binds_v2_mutable_fields_without_accepting_immutable_changes() {
    let operation = operation();
    let evidence = evidence(&operation);
    let message = attested(&operation);
    validate_bound_message(&operation, &evidence, &message).unwrap();
    for offset in [0, 4, 8, 44, 76, 108, 140, 148, 152, 184, 216, 248, 280, 312] {
        let mut changed = message.clone();
        changed[offset] ^= 1;
        assert!(
            validate_bound_message(&operation, &evidence, &changed).is_err(),
            "offset {offset}"
        );
    }
    let mut changed = message.clone();
    changed[12..44].fill(0);
    assert!(validate_bound_message(&operation, &evidence, &changed).is_err());
    changed = message;
    changed[144..148].copy_from_slice(&1000u32.to_be_bytes());
    assert!(validate_bound_message(&operation, &evidence, &changed).is_err());
}

#[test]
fn decodes_trusted_logs_and_rejects_spoofed_ambiguous_or_reorganized_receipts() {
    let operation = operation();
    let valid = receipt(&operation);
    let status = observation(Finality::Included);
    let hash = evidence(&operation).transaction_hash;
    assert_eq!(
        decode_burn_receipt(&operation, &valid, &status, &hash).unwrap(),
        evidence(&operation)
    );
    for change in 0..7 {
        let mut changed = valid.clone();
        match change {
            0 => {
                changed["logs"][0]["address"] =
                    serde_json::json!("0x1111111111111111111111111111111111111111")
            }
            1 => changed["logs"][1]["removed"] = serde_json::json!(true),
            2 => changed["blockHash"] = serde_json::json!(format!("0x{}", "ef".repeat(32))),
            3 => changed["logs"]
                .as_array_mut()
                .unwrap()
                .push(valid["logs"][0].clone()),
            4 => changed["logs"][0]["data"] = serde_json::json!("0x00"),
            5 => changed["status"] = serde_json::json!("0x0"),
            _ => changed["logs"][0]["logIndex"] = serde_json::json!("0x3"),
        }
        assert!(
            decode_burn_receipt(&operation, &changed, &status, &hash).is_err(),
            "mutation {change}"
        );
    }
}

#[test]
fn independent_receipt_and_api_fixtures_bind_to_the_operation() {
    // Static vectors from an independent Python ABI/Keccak encoder.
    let operation = operation();
    let receipt: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/cctp/burn-receipt.json")).unwrap();
    let response: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/cctp/attestation.json")).unwrap();
    let decoded = decode_burn_receipt(
        &operation,
        &receipt,
        &observation(Finality::Included),
        &evidence(&operation).transaction_hash,
    )
    .unwrap();
    assert_eq!(decoded, evidence(&operation));
    let pair = select_attestation(&operation, &decoded, &response)
        .unwrap()
        .unwrap();
    assert_eq!(pair.0, attested(&operation));
    assert_eq!(pair.1, vec![1; 65]);
}

#[test]
fn legacy_attestation_payloads_do_not_imply_binding_or_simulation() {
    let legacy: CctpData =
        serde_json::from_value(serde_json::json!({"message":[1,2],"attestation":[3,4]})).unwrap();
    assert!(!legacy.simulated);
    assert!(legacy.evidence.is_none());
    assert!(legacy.nonce.is_none());
    assert!(legacy.poll.is_none());
    let mut operation = operation();
    operation.attempts[0].tx_status = Some(observation(Finality::Finalized));
    operation.cctp = Some(legacy);
    assert!(crate::operations::advance_operation(&mut operation, OperationStep::Mint).is_err());
}

#[test]
fn polling_preserves_finality_gate_and_restart_backoff() {
    let directory = directory();
    let state = state(&directory);
    let operation = state.lock().unwrap().wallet.as_ref().unwrap().operations[0].clone();
    let provider = mock_provider(&operation, Finality::Included);
    let result = runtime()
        .block_on(poll_attestation_at(&state, &provider, &operation.id, 1000))
        .unwrap();
    assert_eq!(result.state, "waiting_source_finality");
    assert_eq!(result.retry_after_secs, 10);
    let stored = state.lock().unwrap().wallet.as_ref().unwrap().operations[0].clone();
    assert_eq!(stored.step, OperationStep::AwaitingAttestation);
    let mut stored_clone = stored.clone();
    assert!(crate::operations::advance_operation(&mut stored_clone, OperationStep::Mint).is_err());
    runtime()
        .block_on(poll_attestation_at(&state, &provider, &operation.id, 1001))
        .unwrap();
    assert_eq!(provider.calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    // Decode the actual persisted envelope to prove emitted bytes, attested bytes,
    // nonce, simulation gate, and deadline survive an encrypted restart.
    let envelope = crate::storage::read_stored_wallet(&directory.join("wallet.json"))
        .unwrap()
        .unwrap();
    let ciphertext = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &envelope.ciphertext,
    )
    .unwrap();
    let nonce = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &envelope.nonce)
        .unwrap();
    use aes_gcm::{KeyInit, aead::Aead};
    let cipher = aes_gcm::Aes256Gcm::new_from_slice(&[1; 32]).unwrap();
    let plaintext = cipher
        .decrypt(
            &aes_gcm::Nonce::try_from(nonce.as_slice()).unwrap(),
            ciphertext.as_ref(),
        )
        .unwrap();
    let payload: crate::dto::WalletPayload = serde_json::from_slice(&plaintext).unwrap();
    assert_eq!(payload.operations[0], stored);
    let mut guard = state.lock().unwrap();
    guard.wallet.as_mut().unwrap().operations = payload.operations;
    guard.advance_wallet_generation();
    drop(guard);
    runtime()
        .block_on(poll_attestation_at(&state, &provider, &operation.id, 1002))
        .unwrap();
    assert_eq!(provider.calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    let finalized = mock_provider(&operation, Finality::Finalized);
    let result = runtime()
        .block_on(poll_attestation_at(&state, &finalized, &operation.id, 1010))
        .unwrap();
    assert_eq!(result.state, "available");
    let mut operation = state.lock().unwrap().wallet.as_ref().unwrap().operations[0].clone();
    crate::operations::advance_operation(&mut operation, OperationStep::Mint).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

struct MockProvider {
    receipt: Option<(serde_json::Value, TxStatus)>,
    response: Result<MessageResponse, String>,
    simulation: Result<(), String>,
    calls: std::sync::atomic::AtomicUsize,
}

impl AttestationProvider for MockProvider {
    async fn receipt(
        &self,
        _: &CctpTransfer,
        _: &str,
    ) -> Result<Option<(serde_json::Value, TxStatus)>, String> {
        Ok(self.receipt.clone())
    }
    async fn messages(&self, _: &CctpTransfer, _: &str) -> Result<MessageResponse, String> {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.response.clone()
    }
    async fn verify(
        &self,
        _: &CctpTransfer,
        _: &[u8],
        _: &[u8],
    ) -> Result<AttestationCheck, String> {
        self.simulation
            .clone()
            .map(|_| AttestationCheck::DestinationSimulation)
    }
}

fn mock_provider(operation: &Operation, finality: Finality) -> MockProvider {
    MockProvider {
        receipt: Some((receipt(operation), observation(finality))),
        response: Ok(MessageResponse::Messages(response(operation))),
        simulation: Ok(()),
        calls: Default::default(),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn state(directory: &std::path::Path) -> Mutex<AppState> {
    let mut state = AppState::from_storage(directory.join("wallet.json"));
    let operation = operation();
    state.wallet = Some(crate::dto::Wallet {
        name: "Attestation fixture".into(),
        mnemonic: "fixture secret".into(),
        bip39_passphrase: String::new(),
        created_at: String::new(),
        addresses: std::collections::HashMap::from([("evm".into(), operation.sender.clone())]),
        wallet_password_hash: "fixture".into(),
        fiat_currency: crate::dto::FiatCurrency::Usd,
        usd_exchange_rate: 1.,
        assets: vec![],
        activity: vec![],
        operations: vec![operation],
        enabled_networks: vec!["ethereum".into(), "arbitrum_one".into()],
        auto_lock_timeout_secs: None,
        use_crypto_symbols: false,
    });
    state.encryption_key = Some([1; 32]);
    state.storage_salt = Some(vec![2; 16]);
    state.locked = false;
    Mutex::new(state)
}

fn directory() -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "vaultforge-attestation-{}",
        crate::activity::random_hex(16)
    ));
    std::fs::create_dir(&directory).unwrap();
    directory
}

#[test]
fn polling_rejects_invalid_attestations_and_save_failures_without_advancing() {
    let directory = directory();
    let state = state(&directory);
    let operation = state.lock().unwrap().wallet.as_ref().unwrap().operations[0].clone();
    let mut provider = mock_provider(&operation, Finality::Finalized);
    provider.simulation = Err("invalid attestation".into());
    let result = runtime()
        .block_on(poll_attestation_at(&state, &provider, &operation.id, 1000))
        .unwrap();
    assert_eq!(result.state, "error");
    let after = state.lock().unwrap().wallet.as_ref().unwrap().operations[0].clone();
    assert_eq!(after.step, operation.step);
    assert!(after.cctp.as_ref().unwrap().attestation.is_none());
    provider.simulation = Ok(());
    state.lock().unwrap().storage_path = directory.clone();
    assert!(
        runtime()
            .block_on(poll_attestation_at(&state, &provider, &operation.id, 2000))
            .is_err()
    );
    assert_eq!(
        state.lock().unwrap().wallet.as_ref().unwrap().operations[0],
        after
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn polling_retains_pending_and_rate_limit_deadlines() {
    for response in [MessageResponse::Pending, MessageResponse::RateLimited(300)] {
        let directory = directory();
        let state = state(&directory);
        let operation = state.lock().unwrap().wallet.as_ref().unwrap().operations[0].clone();
        let mut provider = mock_provider(&operation, Finality::Included);
        provider.response = Ok(response);
        let result = runtime()
            .block_on(poll_attestation_at(&state, &provider, &operation.id, 1000))
            .unwrap();
        assert_eq!(result.state, "pending");
        assert!(
            state.lock().unwrap().wallet.as_ref().unwrap().operations[0]
                .cctp
                .as_ref()
                .unwrap()
                .evidence
                .is_some()
        );
        runtime()
            .block_on(poll_attestation_at(&state, &provider, &operation.id, 1001))
            .unwrap();
        assert_eq!(provider.calls.load(std::sync::atomic::Ordering::Relaxed), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn selects_matching_logs_and_api_entry_among_unrelated_messages() {
    let operation = operation();
    let mut logs = receipt(&operation);
    let mut unrelated = emitted(&operation);
    unrelated[184] ^= 1;
    let mut abi = Vec::new();
    abi.extend_from_slice(&evm_uint_word(32));
    abi.extend_from_slice(&evm_uint_word(unrelated.len() as u128));
    abi.extend_from_slice(&unrelated);
    abi.resize(64 + unrelated.len().div_ceil(32) * 32, 0);
    let mut log = logs["logs"][0].clone();
    log["data"] = serde_json::json!(format!("0x{}", hex::encode(abi)));
    log["logIndex"] = serde_json::json!("0x0");
    logs["logs"].as_array_mut().unwrap().insert(0, log);
    let evidence = decode_burn_receipt(
        &operation,
        &logs,
        &observation(Finality::Included),
        &evidence(&operation).transaction_hash,
    )
    .unwrap();
    let mut json = response(&operation);
    let mut unrelated = json["messages"][0].clone();
    let mut bytes = attested(&operation);
    bytes[184] ^= 1;
    unrelated["message"] = serde_json::json!(format!("0x{}", hex::encode(bytes)));
    json["messages"]
        .as_array_mut()
        .unwrap()
        .insert(0, unrelated);
    assert_eq!(
        select_attestation(&operation, &evidence, &json)
            .unwrap()
            .unwrap()
            .0,
        attested(&operation)
    );
}

#[test]
fn selects_unique_api_messages_and_rejects_mismatches() {
    let operation = operation();
    let evidence = evidence(&operation);
    let valid = response(&operation);
    assert!(
        select_attestation(&operation, &evidence, &valid)
            .unwrap()
            .is_some()
    );
    for change in 0..5 {
        let mut changed = valid.clone();
        match change {
            0 => changed["sourceTxHash"] = serde_json::json!(format!("0x{}", "ef".repeat(32))),
            1 => changed["messages"]
                .as_array_mut()
                .unwrap()
                .push(valid["messages"][0].clone()),
            2 => changed["messages"][0]["attestation"] = serde_json::json!("0x01"),
            3 => changed["messages"][0]["status"] = serde_json::json!("unknown"),
            _ => changed["messages"][0]["message"] = serde_json::json!("0x00"),
        }
        assert!(select_attestation(&operation, &evidence, &changed).is_err());
    }
    let pending = serde_json::json!({"sourceTxHash":evidence.transaction_hash,"messages":[{"status":"pending_confirmations","message":"0x"}]});
    assert!(
        select_attestation(&operation, &evidence, &pending)
            .unwrap()
            .is_none()
    );
}

#[test]
fn stale_wallet_or_operation_snapshots_never_commit_attestation() {
    struct ChangingProvider<'a> {
        state: &'a Mutex<AppState>,
        inner: MockProvider,
        replace_wallet: bool,
    }
    impl AttestationProvider for ChangingProvider<'_> {
        async fn receipt(
            &self,
            transfer: &CctpTransfer,
            hash: &str,
        ) -> Result<Option<(serde_json::Value, TxStatus)>, String> {
            self.inner.receipt(transfer, hash).await
        }
        async fn messages(&self, _: &CctpTransfer, _: &str) -> Result<MessageResponse, String> {
            let mut state = self.state.lock().unwrap();
            if self.replace_wallet {
                state.advance_wallet_generation();
                state.wallet.as_mut().unwrap().name = "New wallet".into();
            } else {
                state.wallet.as_mut().unwrap().operations[0].last_error = Some("changed".into());
            }
            drop(state);
            self.inner.response.clone()
        }
        async fn verify(
            &self,
            _: &CctpTransfer,
            _: &[u8],
            _: &[u8],
        ) -> Result<AttestationCheck, String> {
            Ok(AttestationCheck::DestinationSimulation)
        }
    }
    for replace_wallet in [false, true] {
        let directory = directory();
        let state = state(&directory);
        let operation = state.lock().unwrap().wallet.as_ref().unwrap().operations[0].clone();
        let provider = ChangingProvider {
            state: &state,
            inner: mock_provider(&operation, Finality::Finalized),
            replace_wallet,
        };
        assert!(
            runtime()
                .block_on(poll_attestation_at(&state, &provider, &operation.id, 1000))
                .is_err()
        );
        assert!(!directory.join("wallet.json").exists());
        assert!(
            state.lock().unwrap().wallet.as_ref().unwrap().operations[0]
                .cctp
                .is_none()
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
