use std::borrow::Cow;

use base64::Engine;
use solana_keypair::Keypair;
use solana_signer::Signer;

use super::{LiveNative, decode_mint_transaction, validate_token_account};
use crate::cctp::CctpTransfer;
use crate::dto::AssetIdentity;
use crate::operations::SolanaLookup;
use crate::providers::cctp::tests::MockRpc;
use crate::tx::cctp_solana::{discriminator, key, pda};
use crate::tx::tx_status::{Execution, Finality, TxStatus};

fn operation() -> crate::operations::Operation {
    crate::operations::create_operation(
        AssetIdentity::new(
            "ethereum",
            Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        ),
        AssetIdentity::new(
            "solana",
            Some("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
        ),
        "0x2222222222222222222222222222222222222222".into(),
        Keypair::new_from_array([1; 32]).pubkey().to_string(),
        1000000,
    )
    .unwrap()
}
fn account(owner: &str, data: &[u8]) -> serde_json::Value {
    serde_json::json!({"owner":owner,"executable":false,"data":[base64::engine::general_purpose::STANDARD.encode(data),"base64"]})
}
fn fixture() -> (
    crate::operations::Operation,
    String,
    serde_json::Value,
    TxStatus,
) {
    let mut operation = operation();
    operation.cctp = Some(crate::cctp_attestation::tests::bound_data(&operation));
    let transfer = operation.cctp_transfer().unwrap();
    let message = &operation.cctp.as_ref().unwrap().message;
    let mut received = vec![0x1d, 0x9a, 0xcb, 0x51, 0x2e, 0xa5, 0x45, 0xe4];
    received.extend_from_slice(&discriminator("event:MessageReceived"));
    received.extend_from_slice(&key(&operation.recipient).unwrap().to_bytes());
    received.extend_from_slice(&0u32.to_le_bytes());
    received.extend_from_slice(&message[12..44]);
    received.extend_from_slice(&message[44..76]);
    received.extend_from_slice(&2000u32.to_le_bytes());
    received.extend_from_slice(&228u32.to_le_bytes());
    received.extend_from_slice(&message[148..]);
    let mut minted = vec![0x1d, 0x9a, 0xcb, 0x51, 0x2e, 0xa5, 0x45, 0xe4];
    minted.extend_from_slice(&discriminator("event:MintAndWithdraw"));
    minted.extend_from_slice(&transfer.mint_recipient_word().unwrap());
    minted.extend_from_slice(&1000000u64.to_le_bytes());
    minted.extend_from_slice(&key(transfer.route.destination.usdc).unwrap().to_bytes());
    minted.extend_from_slice(&0u64.to_le_bytes());
    let mt = crate::cctp::SOLANA_TRANSMITTER;
    let tm = crate::cctp::SOLANA_MESSENGER;
    let hash = bs58::encode([7; 64]).into_string();
    let transaction = serde_json::json!({"slot":42,"transaction":{"signatures":[hash],"message":{"accountKeys":[mt,tm,pda(&key(mt).unwrap(),&[b"__event_authority"]).to_string(),pda(&key(tm).unwrap(),&[b"__event_authority"]).to_string()]}},"meta":{"err":null,"innerInstructions":[{"index":0,"instructions":[{"programIdIndex":0,"accounts":[2],"stackHeight":2,"data":bs58::encode(received).into_string()},{"programIdIndex":1,"accounts":[3],"stackHeight":3,"data":bs58::encode(minted).into_string()}]}]}});
    let mut status = TxStatus::new(Execution::Succeeded, Finality::Finalized);
    status.slot = Some(42);
    (operation, hash, transaction, status)
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn decodes_finalized_native_message_and_mint_events() {
    let (o, h, t, s) = fixture();
    let proof = decode_mint_transaction(&o, &h, &t, &s).unwrap();
    assert_eq!(proof.transaction_hash, h);
    assert_eq!(proof.observation.slot, Some(42));
}
#[test]
fn rejects_forged_events_changed_slot_and_unfinalized_execution() {
    for mode in 0..4 {
        let (o, h, mut t, mut s) = fixture();
        match mode {
            0 => {
                t["meta"]["innerInstructions"][0]["instructions"][1]["accounts"][0] = 2.into();
            }
            1 => {
                t["slot"] = 43.into();
            }
            2 => {
                s.finality = Finality::Included;
            }
            _ => {
                t["meta"]["innerInstructions"][0]["instructions"][1]["stackHeight"] = 1.into();
            }
        }
        assert!(decode_mint_transaction(&o, &h, &t, &s).is_err());
    }
}
#[test]
fn rejects_wrong_frozen_and_underfunded_classic_token_accounts() {
    let o = operation();
    let t = o.cctp_transfer().unwrap();
    let mut data = vec![0; 165];
    data[..32].copy_from_slice(&key(t.route.destination.usdc).unwrap().to_bytes());
    data[32..64].copy_from_slice(&key(t.recipient()).unwrap().to_bytes());
    data[64..72].copy_from_slice(&1000000u64.to_le_bytes());
    data[108] = 1;
    let valid = account(&spl_token_interface::ID.to_string(), &data);
    validate_token_account(&valid, t.route.destination.usdc, t.recipient(), 1000000).unwrap();
    assert!(
        validate_token_account(&valid, t.route.destination.usdc, t.recipient(), 1000001).is_err()
    );
    data[108] = 2;
    assert!(
        validate_token_account(
            &account(&spl_token_interface::ID.to_string(), &data),
            t.route.destination.usdc,
            t.recipient(),
            1
        )
        .is_err()
    );
    assert!(
        validate_token_account(
            &account(crate::cctp::SOLANA_MESSENGER, &data),
            t.route.destination.usdc,
            t.recipient(),
            1
        )
        .is_err()
    );
}
#[test]
fn validates_real_lookup_owner_addresses_activation_and_freeze() {
    use solana_address_lookup_table_interface::state::{AddressLookupTable, LookupTableMeta};
    for mode in 0..4 {
        let address = Keypair::new_from_array([2; 32]).pubkey();
        let expected = vec![Keypair::new_from_array([3; 32]).pubkey()];
        let mut meta = LookupTableMeta::default();
        if mode == 1 {
            meta.deactivation_slot = 40;
        }
        if mode == 2 {
            meta.last_extended_slot = 50;
        }
        if mode == 3 {
            meta.authority = Some(address);
        }
        let data = AddressLookupTable {
            meta,
            addresses: Cow::Borrowed(&expected),
        }
        .serialize_for_tests()
        .unwrap();
        let response = account("AddressLookupTab1e1111111111111111111111111", &data);
        let server = MockRpc::new(
            move |_| serde_json::json!({"jsonrpc":"2.0","id":1,"result":{"context":{"slot":50},"value":response}}),
        );
        let mut transfer: CctpTransfer = operation().cctp_transfer().unwrap();
        transfer.route.destination.network.rpc_url = Some(server.url.clone());
        let lookup = SolanaLookup {
            address: address.to_string(),
            addresses: expected.iter().map(ToString::to_string).collect(),
        };
        let client = reqwest::Client::new();
        assert_eq!(
            runtime()
                .block_on(LiveNative { client: &client }.lookup(&transfer, &lookup))
                .is_ok(),
            mode == 0
        );
    }
}
