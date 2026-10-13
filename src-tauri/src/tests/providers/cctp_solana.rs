use super::*;
use crate::dto::AssetIdentity;
use crate::operations::{TransactionStatus, TransactionStep};
use crate::providers::cctp::tests::MockRpc;
use crate::tx::tx_status::Finality;

const SENDER: &str = "Vote111111111111111111111111111111111111111";
const RECIPIENT: &str = "0x4444444444444444444444444444444444444444";
fn source_operation() -> Operation {
    let mut operation = crate::operations::create_operation(
        AssetIdentity::new(
            "solana",
            Some("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
        ),
        AssetIdentity::new(
            "ethereum",
            Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        ),
        SENDER.into(),
        RECIPIENT.into(),
        1000000,
    )
    .unwrap();
    let mut attempt =
        crate::tests::sample_attempt(TransactionStep::Burn, TransactionStatus::Submitted);
    attempt.tx_hash = bs58::encode([1; 64]).into_string();
    operation.attempts.push(attempt);
    operation
}
fn proof(operation: &Operation) -> serde_json::Value {
    let transfer = operation.cctp_transfer().unwrap();
    let emitted = crate::cctp_attestation::tests::emitted(operation);
    let account = Pubkey::new_from_array([3; 32]).to_string();
    let mut data = Vec::from(discriminator("account:MessageSent"));
    data.extend_from_slice(&pubkey(SENDER).unwrap().to_bytes());
    data.extend_from_slice(&123i64.to_le_bytes());
    data.extend_from_slice(&376u32.to_le_bytes());
    data.extend_from_slice(&emitted);
    let mut deposit = Vec::from(discriminator("global:deposit_for_burn"));
    deposit.extend_from_slice(&1000000u64.to_le_bytes());
    deposit.extend_from_slice(&0u32.to_le_bytes());
    deposit.extend_from_slice(&transfer.mint_recipient_word().unwrap());
    deposit.extend_from_slice(&transfer.caller_word().unwrap());
    deposit.extend_from_slice(&0u64.to_le_bytes());
    deposit.extend_from_slice(&2000u32.to_le_bytes());
    let mut send = Vec::from(discriminator("global:send_message"));
    send.extend_from_slice(&0u32.to_le_bytes());
    send.extend_from_slice(
        &address_word(
            &transfer.route.destination.network,
            transfer.route.destination.token_messenger,
        )
        .unwrap(),
    );
    send.extend_from_slice(&transfer.caller_word().unwrap());
    send.extend_from_slice(&2000u32.to_le_bytes());
    send.extend_from_slice(&228u32.to_le_bytes());
    send.extend_from_slice(&emitted[148..]);
    let keys = vec![
        SENDER.to_string(),
        account.clone(),
        transfer.route.source.usdc.into(),
        SOLANA_TRANSMITTER.into(),
        SOLANA_MESSENGER.into(),
    ];
    serde_json::json!({"message_account_address":account,"message_account":{"owner":SOLANA_TRANSMITTER,"executable":false,"data":[base64::engine::general_purpose::STANDARD.encode(data),"base64"]},"transaction":{
        "slot":42,"blockTime":123,"meta":{"err":null,"innerInstructions":[{"index":0,"instructions":[{"programIdIndex":3,"accounts":[0,0,0,1,4,0],"data":bs58::encode(send).into_string()}]}]},
        "transaction":{"signatures":[operation.attempts[0].tx_hash,bs58::encode([2;64]).into_string()],"message":{"header":{"numRequiredSignatures":2},"accountKeys":keys,"instructions":[{"programIdIndex":4,"accounts":[0,0,0,0,0,0,0,0,0,0,2,1,3,4,0,0],"data":bs58::encode(deposit).into_string()}]}}
    }})
}
fn status() -> TxStatus {
    let mut status = TxStatus::new(Execution::Succeeded, Finality::Finalized);
    status.slot = Some(42);
    status
}
fn signed_attestation(message: &[u8]) -> (Vec<u8>, [u8; 32]) {
    use k256::ecdsa::signature::hazmat::PrehashSigner;
    let key = k256::ecdsa::SigningKey::from_slice(&[7; 32]).unwrap();
    let digest = Keccak256::digest(message);
    let signature: k256::ecdsa::Signature = key.sign_prehash(&digest).unwrap();
    let mut bytes = signature.to_bytes().to_vec();
    for parity in 0..4 {
        let id = k256::ecdsa::RecoveryId::from_byte(parity).unwrap();
        if k256::ecdsa::VerifyingKey::recover_from_prehash(&digest, &signature, id)
            .is_ok_and(|public| &public == key.verifying_key())
        {
            bytes.push(parity + 27);
            break;
        }
    }
    assert_eq!(bytes.len(), 65);
    let public = key.verifying_key().to_sec1_point(false);
    let hash = Keccak256::digest(&public.as_bytes()[1..]);
    let mut address = [0; 32];
    address[12..].copy_from_slice(&hash[12..]);
    (bytes, address)
}
fn attester_state(address: &[u8; 32]) -> Vec<u8> {
    let mut state = Vec::from(discriminator("account:MessageTransmitter"));
    state.resize(136, 1);
    state.push(0);
    state.extend_from_slice(&5u32.to_le_bytes());
    state.extend_from_slice(&1u32.to_le_bytes());
    state.extend_from_slice(&1u32.to_le_bytes());
    state.extend_from_slice(&1u32.to_le_bytes());
    state.extend_from_slice(address);
    state.extend_from_slice(&16384u64.to_le_bytes());
    state
}

#[test]
fn binds_native_solana_deposits_and_message_accounts_without_evm_block_fields() {
    let operation = source_operation();
    let proof = proof(&operation);
    let signature = &operation.attempts[0].tx_hash;
    let evidence = decode_source_evidence(&operation, &proof, &status(), signature).unwrap();
    assert_eq!(evidence.slot, Some(42));
    assert!(evidence.block_hash.is_none());
    assert!(evidence.block_height.is_none());
    let message = crate::cctp_attestation::tests::attested(&operation);
    crate::cctp_attestation::validate_bound_message(&operation, &evidence, &message).unwrap();
    for mutation in 0..7 {
        let mut bad = proof.clone();
        match mutation {
            0 => bad["message_account"]["owner"] = serde_json::json!(SOLANA_MESSENGER),
            1 => {
                bad["transaction"]["meta"]["err"] =
                    serde_json::json!({"InstructionError":[0,"failed"]})
            }
            2 => bad["transaction"]["slot"] = serde_json::json!(43),
            3 => {
                bad["transaction"]["transaction"]["message"]["instructions"][0]["accounts"][0] =
                    serde_json::json!(2)
            }
            4 => {
                bad["transaction"]["meta"]["innerInstructions"][0]["instructions"][0]["accounts"]
                    [4] = serde_json::json!(3)
            }
            5 => bad["transaction"]["meta"]
                .as_object_mut()
                .unwrap()
                .remove("err")
                .map(|_| ())
                .unwrap(),
            _ => bad["transaction"]["transaction"]["message"]["instructions"]
                .as_array_mut()
                .unwrap()
                .push(proof["transaction"]["transaction"]["message"]["instructions"][0].clone()),
        }
        assert!(decode_source_evidence(&operation, &bad, &status(), signature).is_err());
    }
}

#[test]
fn rejects_disabled_tampered_duplicate_and_wrong_threshold_attestations() {
    let message = vec![5; 376];
    let (attestation, address) = signed_attestation(&message);
    let state = attester_state(&address);
    verify_attesters(&state, &message, &attestation).unwrap();
    let mut bad = attestation.clone();
    bad[0] ^= 1;
    assert!(verify_attesters(&state, &message, &bad).is_err());
    let mut changed = message.clone();
    changed[0] ^= 1;
    assert!(verify_attesters(&state, &changed, &attestation).is_err());
    let mut disabled = state.clone();
    disabled[153..185].fill(0);
    assert!(verify_attesters(&disabled, &message, &attestation).is_err());
    let mut threshold = state.clone();
    threshold[145..149].copy_from_slice(&2u32.to_le_bytes());
    assert!(verify_attesters(&threshold, &message, &attestation).is_err());
    let mut duplicate = attestation.clone();
    duplicate.extend_from_slice(&attestation);
    assert!(verify_attesters(&state, &message, &duplicate).is_err());
}

#[test]
fn validates_on_chain_attester_account_ownership_and_solana_genesis() {
    let route = crate::cctp::CctpRoute::for_networks("ethereum", "solana").unwrap();
    let transfer = CctpTransfer::new(
        &AssetIdentity::new("ethereum", Some(route.source.usdc)),
        &AssetIdentity::new("solana", Some(route.destination.usdc)),
        RECIPIENT,
        SENDER,
        1000000,
    )
    .unwrap();
    let message = vec![5; 376];
    let (signature, address) = signed_attestation(&message);
    let encoded = base64::engine::general_purpose::STANDARD.encode(attester_state(&address));
    let server = MockRpc::new(move |request| match request["method"].as_str().unwrap() {
        "getGenesisHash" => {
            serde_json::json!({"result":"5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d"})
        }
        "getAccountInfo" => {
            serde_json::json!({"result":{"context":{"slot":42},"value":{"owner":SOLANA_TRANSMITTER,"executable":false,"data":[encoded,"base64"]}}})
        }
        method => panic!("Unexpected method {method}"),
    });
    let mut transfer = transfer;
    transfer.route.destination.network.rpc_url = Some(server.url.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime
        .block_on(verify_destination_attesters(
            &reqwest::Client::new(),
            &transfer,
            &message,
            &signature,
        ))
        .unwrap();
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}
