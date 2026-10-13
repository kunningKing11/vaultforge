use sha3::{Digest, Keccak256};

use super::decode_mint_receipt;
use crate::cctp_attestation::tests::{bound_data, observation};
use crate::tests::sample_operation;
use crate::tx::evm::{evm_address_word, evm_uint_word};
use crate::tx::tx_status::Finality;

fn topic(signature: &str) -> String {
    format!("0x{}", hex::encode(Keccak256::digest(signature.as_bytes())))
}
fn hex(bytes: impl AsRef<[u8]>) -> String {
    format!("0x{}", hex::encode(bytes))
}
fn fixture() -> (
    crate::operations::Operation,
    String,
    serde_json::Value,
    crate::tx::tx_status::TxStatus,
) {
    let mut operation = sample_operation();
    operation.cctp = Some(bound_data(&operation));
    let transfer = operation.cctp_transfer().unwrap();
    let message = &operation.cctp.as_ref().unwrap().message;
    let status = observation(Finality::Finalized);
    let hash = format!("0x{}", "99".repeat(32));
    let mut received = evm_uint_word(transfer.route.source.domain as u128).to_vec();
    received.extend_from_slice(&message[44..76]);
    received.extend_from_slice(&evm_uint_word(96));
    received.extend_from_slice(&evm_uint_word(228));
    received.extend_from_slice(&message[148..]);
    received.resize(384, 0);
    let mut minted = evm_uint_word(transfer.amount()).to_vec();
    minted.extend_from_slice(&evm_uint_word(0));
    let receipt = serde_json::json!({"transactionHash":hash,"blockHash":status.block_hash,"blockNumber":"0x2a","status":"0x1","logs":[
        {"address":transfer.route.destination.message_transmitter,"removed":false,"topics":[topic("MessageReceived(address,uint32,bytes32,bytes32,uint32,bytes)"),hex(evm_address_word(transfer.destination_caller()).unwrap()),hex(&message[12..44]),hex(evm_uint_word(2000))],"data":hex(received)},
        {"address":transfer.route.destination.token_messenger,"removed":false,"topics":[topic("MintAndWithdraw(address,uint256,address,uint256)"),hex(evm_address_word(transfer.recipient()).unwrap()),hex(evm_address_word(transfer.route.destination.usdc).unwrap())],"data":hex(minted)}]});
    (operation, hash, receipt, status)
}

#[test]
fn broadcast_fails_over_to_secondary_endpoints() {
    use crate::providers::http::tests::{runtime, status_server};

    let (primary, primary_hits, primary_handle) = status_server(500, "{}", 3);
    let (fallback, fallback_hits, fallback_handle) =
        status_server(200, "{\"result\":\"0xabc\"}", 1);
    let hash = runtime()
        .block_on(super::broadcast_once_at(
            &reqwest::Client::new(),
            &[primary, fallback],
            "0xdeadbeef",
        ))
        .unwrap();
    assert_eq!(hash, "0xabc");
    assert_eq!(*primary_hits.lock().unwrap(), 3);
    assert_eq!(*fallback_hits.lock().unwrap(), 1);
    primary_handle.join().unwrap();
    fallback_handle.join().unwrap();
}
#[test]
fn broadcast_reports_error_when_all_endpoints_fail() {
    use crate::providers::http::tests::{runtime, status_server};

    let (primary, _, primary_handle) = status_server(500, "{}", 3);
    let (fallback, _, fallback_handle) = status_server(500, "{}", 3);
    let error = runtime()
        .block_on(super::broadcast_once_at(
            &reqwest::Client::new(),
            &[primary, fallback],
            "0xdeadbeef",
        ))
        .unwrap_err();
    assert!(error.contains("attempt 3/3"), "{error}");
    primary_handle.join().unwrap();
    fallback_handle.join().unwrap();
}
#[test]
fn broadcast_stops_at_deterministic_rpc_errors() {
    use crate::providers::http::tests::{runtime, status_server};

    let (primary, _, primary_handle) = status_server(
        200,
        "{\"error\":{\"code\":-32000,\"message\":\"already known\"}}",
        1,
    );
    let (_, fallback_hits, _) = status_server(200, "{\"result\":\"0xabc\"}", 0);
    let error = runtime()
        .block_on(super::broadcast_once_at(
            &reqwest::Client::new(),
            &[primary, "http://127.0.0.1:1/unused".to_string()],
            "0xdeadbeef",
        ))
        .unwrap_err();
    assert!(error.contains("already known"), "{error}");
    assert_eq!(*fallback_hits.lock().unwrap(), 0);
    primary_handle.join().unwrap();
}
#[test]
fn decodes_finalized_message_and_mint_pair() {
    let (o, h, r, s) = fixture();
    let proof = decode_mint_receipt(&o, &h, &r, &s).unwrap();
    assert_eq!(proof.transaction_hash, h);
    assert_eq!(
        proof.message_hash,
        hex::encode(Keccak256::digest(&o.cctp.unwrap().message))
    );
}

#[test]
fn rejects_changed_body_caller_nonce_and_token() {
    for (log, field, index) in [
        (0, "data", 0),
        (0, "topics", 1),
        (0, "topics", 2),
        (1, "topics", 1),
        (1, "topics", 2),
    ] {
        let (o, h, mut r, s) = fixture();
        if field == "data" {
            r["logs"][log][field] = serde_json::Value::String(hex([0; 384]));
        } else {
            r["logs"][log][field][index] = serde_json::Value::String(hex([0; 32]));
        }
        assert!(decode_mint_receipt(&o, &h, &r, &s).is_err());
    }
}

#[test]
fn rejects_duplicate_missing_or_untrusted_events() {
    for mode in 0..3 {
        let (o, h, mut r, s) = fixture();
        match mode {
            0 => {
                let log = r["logs"][0].clone();
                r["logs"].as_array_mut().unwrap().push(log);
            }
            1 => {
                r["logs"].as_array_mut().unwrap().pop();
            }
            _ => r["logs"][1]["address"] = "0x0000000000000000000000000000000000000001".into(),
        };
        assert!(decode_mint_receipt(&o, &h, &r, &s).is_err());
    }
}

#[test]
fn rejects_unfinalized_failed_and_changed_receipts() {
    let (o, h, r, mut s) = fixture();
    s.finality = Finality::Included;
    assert!(decode_mint_receipt(&o, &h, &r, &s).is_err());
    let (o, h, mut r, s) = fixture();
    r["status"] = "0x0".into();
    assert!(decode_mint_receipt(&o, &h, &r, &s).is_err());
    let (o, h, mut r, s) = fixture();
    r["blockHash"] = hex([0; 32]).into();
    assert!(decode_mint_receipt(&o, &h, &r, &s).is_err());
}
