use super::*;
use serde_json::{Value, json};

pub(crate) fn mint_bytes(decimals: u8) -> Vec<u8> {
    let mut bytes = vec![0; 82];
    bytes[44] = decimals;
    bytes[45] = 1;
    bytes
}
pub(crate) fn holding_bytes(mint: [u8; 32], owner: [u8; 32], amount: u64, frozen: bool) -> Vec<u8> {
    let mut bytes = vec![0; 165];
    bytes[..32].copy_from_slice(&mint);
    bytes[32..64].copy_from_slice(&owner);
    bytes[64..72].copy_from_slice(&amount.to_le_bytes());
    bytes[108] = if frozen { 2 } else { 1 };
    bytes
}
pub(crate) fn extend(bytes: &mut Vec<u8>, kind: u8, extension: ExtensionType, payload: &[u8]) {
    bytes.resize(bytes.len().max(166), 0);
    bytes[165] = kind;
    bytes.extend_from_slice(&u16::from(extension).to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    bytes.extend_from_slice(payload);
}
pub(crate) fn value(program: &str, bytes: &[u8]) -> Value {
    json!({"owner":program,"executable":false,"data":[STANDARD.encode(bytes),"base64"]})
}

#[test]
fn derives_program_specific_addresses_and_account_sizes() {
    let mint = Pubkey::from([7; 32]).to_string();
    let owner = Pubkey::from([8; 32]).to_string();
    let classic = inspect_mint(&mint, &value(CLASSIC, &mint_bytes(6))).unwrap();
    let modern = inspect_mint(&mint, &value(TOKEN_2022, &mint_bytes(6))).unwrap();
    assert_ne!(
        classic.associated_address(&owner).unwrap(),
        modern.associated_address(&owner).unwrap()
    );
    assert_eq!(classic.account_len().unwrap(), 165);
    assert_eq!(modern.account_len().unwrap(), 170);
    assert!(classic.associated_address("bad").is_err());
}

#[test]
fn inspects_public_balances_without_adding_withheld_fees() {
    let mint = Pubkey::from([7; 32]).to_string();
    let owner = Pubkey::from([8; 32]).to_string();
    let mut bytes = holding_bytes([7; 32], [8; 32], 100, true);
    extend(
        &mut bytes,
        2,
        ExtensionType::TransferFeeAmount,
        &500u64.to_le_bytes(),
    );
    let holding = inspect_holding(&mint, &owner, &value(TOKEN_2022, &bytes)).unwrap();
    assert_eq!(holding.amount, 100);
    assert_eq!(
        holding.unsupported_reason().unwrap(),
        "Solana token account is frozen"
    );
    assert!(inspect_holding(&mint, &mint, &value(TOKEN_2022, &bytes)).is_err());
}

#[test]
fn rejects_malformed_extension_payloads_and_unknown_programs() {
    let mint = Pubkey::from([7; 32]).to_string();
    assert!(inspect_mint(&mint, &value("unknown", &mint_bytes(6))).is_err());
    let mut bytes = mint_bytes(6);
    extend(&mut bytes, 1, ExtensionType::TransferFeeConfig, &[0; 10]);
    assert!(inspect_mint(&mint, &value(TOKEN_2022, &bytes)).is_err());
    let mut bytes = mint_bytes(6);
    extend(&mut bytes, 1, ExtensionType::MintCloseAuthority, &[0; 32]);
    extend(&mut bytes, 1, ExtensionType::MintCloseAuthority, &[0; 32]);
    assert!(inspect_mint(&mint, &value(TOKEN_2022, &bytes)).is_err());
    assert!(inspect_mint(&mint, &value(CLASSIC, &bytes)).is_err());
    let mut bytes = mint_bytes(6);
    extend(&mut bytes, 1, ExtensionType::TokenMetadata, &[0; 10]);
    assert!(inspect_mint(&mint, &value(TOKEN_2022, &bytes)).is_err());
    let mut bytes = mint_bytes(6);
    bytes[45] = 0;
    assert!(inspect_mint(&mint, &value(TOKEN_2022, &bytes)).is_err());
}

#[test]
fn retains_fee_schedules_and_uses_epoch_rounding_and_caps() {
    let mint = Pubkey::from([7; 32]).to_string();
    let mut bytes = mint_bytes(6);
    let mut fee = vec![0; 108];
    // authorities(64), withheld amount(8), older schedule(18), newer schedule(18)
    fee[80..88].copy_from_slice(&10u64.to_le_bytes());
    fee[88..90].copy_from_slice(&100u16.to_le_bytes());
    fee[90..98].copy_from_slice(&5u64.to_le_bytes());
    fee[98..106].copy_from_slice(&20u64.to_le_bytes());
    fee[106..108].copy_from_slice(&200u16.to_le_bytes());
    extend(&mut bytes, 1, ExtensionType::TransferFeeConfig, &fee);
    let descriptor = inspect_mint(&mint, &value(TOKEN_2022, &bytes)).unwrap();
    assert_eq!(descriptor.decimals, 6);
    assert_eq!(
        descriptor.required_account_extensions,
        vec![ExtensionType::TransferFeeAmount]
    );
    assert_eq!(descriptor.account_len().unwrap(), 182);
    assert_eq!(descriptor.fee(4, 1).unwrap(), 1);
    assert_eq!(descriptor.fee(4, 10_000).unwrap(), 10);
    assert_eq!(descriptor.fee(5, 10_000).unwrap(), 20);
    assert!(descriptor.unsupported_reason().is_none());
}

#[test]
fn separates_passive_extensions_from_unsupported_spending() {
    let mint = Pubkey::from([7; 32]).to_string();
    let mut bytes = mint_bytes(6);
    extend(&mut bytes, 1, ExtensionType::MintCloseAuthority, &[0; 32]);
    assert!(
        inspect_mint(&mint, &value(TOKEN_2022, &bytes))
            .unwrap()
            .unsupported_reason()
            .is_none()
    );
    extend(&mut bytes, 1, ExtensionType::TransferHook, &[0; 64]);
    assert!(
        inspect_mint(&mint, &value(TOKEN_2022, &bytes))
            .unwrap()
            .unsupported_reason()
            .unwrap()
            .contains("TransferHook")
    );
}
