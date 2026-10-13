use base64::Engine;
use solana_keypair::Keypair;
use solana_signer::Signer;

use super::{
    burn_instruction, compile, key, mint_instruction, provisional_message, setup_instructions,
    signed,
};
use crate::cctp::CctpTransfer;
use crate::dto::AssetIdentity;

fn transfer(source_solana: bool, payer: &Keypair) -> CctpTransfer {
    let evm = AssetIdentity::new(
        "ethereum",
        Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
    );
    let sol = AssetIdentity::new(
        "solana",
        Some("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
    );
    if source_solana {
        CctpTransfer::new(
            &sol,
            &evm,
            &payer.pubkey().to_string(),
            "0x4444444444444444444444444444444444444444",
            1000000,
        )
        .unwrap()
    } else {
        CctpTransfer::new(
            &evm,
            &sol,
            "0x2222222222222222222222222222222222222222",
            &payer.pubkey().to_string(),
            1000000,
        )
        .unwrap()
    }
}
#[test]
fn burn_matches_anchor_layout_and_requires_event_signature() {
    let payer = Keypair::new_from_array([1; 32]);
    let event = Keypair::new_from_array([2; 32]);
    let transfer = transfer(true, &payer);
    let instruction = burn_instruction(&transfer, event.pubkey()).unwrap();
    assert_eq!(hex::encode(&instruction.data[..8]), "d73c3d2e723780b0");
    assert_eq!(instruction.data.len(), 96);
    assert_eq!(&instruction.data[8..16], &1000000u64.to_le_bytes());
    assert_eq!(&instruction.data[16..20], &0u32.to_le_bytes());
    assert_eq!(&instruction.data[52..84], &[0; 32]);
    assert_eq!(&instruction.data[84..92], &[0; 8]);
    assert_eq!(&instruction.data[92..96], &2000u32.to_le_bytes());
    assert_eq!(instruction.accounts.len(), 18);
    assert!(instruction.accounts[11].is_signer);
    assert_eq!(instruction.accounts[11].pubkey, event.pubkey());
    let message = compile(
        payer.pubkey(),
        &[instruction],
        &bs58::encode([3; 32]).into_string(),
        &[],
    )
    .unwrap();
    assert!(signed(message.clone(), &payer, None).is_err());
    assert!(signed(message, &payer, Some(&event)).is_ok());
}
#[test]
fn lookup_setup_keeps_large_mint_within_packet_limit() {
    let payer = Keypair::new_from_array([1; 32]);
    let transfer = transfer(false, &payer);
    let message = provisional_message(&transfer).unwrap();
    let attestation = [1; 325];
    let instruction = mint_instruction(
        &transfer,
        payer.pubkey(),
        payer.pubkey(),
        &message,
        &attestation,
    )
    .unwrap();
    assert_eq!(hex::encode(&instruction.data[..8]), "26907fe11fe1ee19");
    assert_eq!(&instruction.data[8..12], &376u32.to_le_bytes());
    assert_eq!(&instruction.data[388..392], &325u32.to_le_bytes());
    let blockhash = bs58::encode([3; 32]).into_string();
    let legacy = compile(payer.pubkey(), &[instruction.clone()], &blockhash, &[]).unwrap();
    assert!(signed(legacy, &payer, None).is_err());
    let (setup, lookup) =
        setup_instructions(&transfer, payer.pubkey(), payer.pubkey(), 42, true).unwrap();
    let setup = compile(payer.pubkey(), &setup, &blockhash, &[]).unwrap();
    assert!(signed(setup, &payer, None).is_ok());
    let table = solana_message::AddressLookupTableAccount {
        key: key(&lookup.address).unwrap(),
        addresses: lookup
            .addresses
            .iter()
            .map(|address| key(address).unwrap())
            .collect(),
    };
    let versioned = compile(payer.pubkey(), &[instruction], &blockhash, &[table]).unwrap();
    let (_, raw) = signed(versioned, &payer, None).unwrap();
    assert!(
        base64::engine::general_purpose::STANDARD
            .decode(raw)
            .unwrap()
            .len()
            <= 1232
    );
}
#[test]
fn native_mint_accounts_use_recipient_ata_and_permissionless_wallet_payer() {
    let payer = Keypair::new_from_array([1; 32]);
    let transfer = transfer(false, &payer);
    let message = provisional_message(&transfer).unwrap();
    let instruction = mint_instruction(
        &transfer,
        payer.pubkey(),
        payer.pubkey(),
        &message,
        &[1; 130],
    )
    .unwrap();
    assert_eq!(instruction.accounts[0].pubkey, payer.pubkey());
    assert_eq!(instruction.accounts[1].pubkey, payer.pubkey());
    assert!(instruction.accounts[0].is_signer);
    assert!(instruction.accounts[0].is_writable);
    assert_eq!(
        instruction.accounts[15].pubkey.to_bytes(),
        transfer.mint_recipient_word().unwrap()
    );
    assert_ne!(instruction.accounts[15].pubkey, payer.pubkey());
    assert_eq!(instruction.accounts.len(), 20);
    assert_eq!(&message[108..140], &[0; 32]);
}
