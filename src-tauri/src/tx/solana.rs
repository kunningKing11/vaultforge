use std::str::FromStr;

use base64::Engine;
#[allow(deprecated)]
use solana_sdk::{
    hash::Hash,
    instruction::Instruction,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer, keypair_from_seed},
    system_instruction,
    transaction::Transaction,
};
use spl_associated_token_account::{
    get_associated_token_address, instruction::create_associated_token_account_idempotent,
};
use spl_token::instruction::transfer_checked;

use crate::derivation::solana_secret_key_from_mnemonic;
use crate::providers::solana::{fetch_latest_solana_blockhash, fetch_solana_fee_for_message};

pub(crate) struct SignedSolanaTransfer {
    pub(crate) signature: String,
    pub(crate) raw_tx_base64: String,
    pub(crate) recent_blockhash: String,
    pub(crate) fee_lamports: u64,
}

pub(crate) struct SolanaTransferDraft<'a> {
    pub(crate) mnemonic: &'a str,
    pub(crate) from: &'a str,
    pub(crate) recent_blockhash: &'a str,
    pub(crate) fee_lamports: u64,
    pub(crate) instructions: Vec<Instruction>,
}

fn solana_keypair_from_mnemonic(mnemonic: &str) -> Result<Keypair, String> {
    let secret = solana_secret_key_from_mnemonic(mnemonic)?;
    keypair_from_seed(&secret).map_err(|_| "Failed to create Solana keypair".to_string())
}

pub(crate) fn sign_solana_transfer_with_blockhash(
    mnemonic: &str,
    from: &str,
    to: &str,
    lamports: u64,
    recent_blockhash: &str,
    fee_lamports: u64,
) -> Result<SignedSolanaTransfer, String> {
    let instructions = native_transfer_instructions(from, to, lamports)?;
    sign_solana_instructions(SolanaTransferDraft {
        mnemonic,
        from,
        recent_blockhash,
        fee_lamports,
        instructions,
    })
}

pub(crate) fn sign_solana_token_transfer_with_blockhash(
    mnemonic: &str,
    from: &str,
    to: &str,
    mint: &str,
    amount: u64,
    decimals: u8,
    recent_blockhash: &str,
    fee_lamports: u64,
) -> Result<SignedSolanaTransfer, String> {
    let instructions = spl_token_transfer_instructions(from, to, mint, amount, decimals)?;

    sign_solana_instructions(SolanaTransferDraft {
        mnemonic,
        from,
        recent_blockhash,
        fee_lamports,
        instructions,
    })
}

fn sign_solana_instructions(draft: SolanaTransferDraft) -> Result<SignedSolanaTransfer, String> {
    let from_pubkey = parse_pubkey(draft.from, "from")?;
    let blockhash = parse_blockhash(draft.recent_blockhash)?;
    let keypair = solana_keypair_from_mnemonic(draft.mnemonic)?;

    if keypair.pubkey() != from_pubkey {
        return Err("Solana signing key does not match from address".to_string());
    }

    let message = Message::new(&draft.instructions, Some(&from_pubkey));
    let mut transaction = Transaction::new_unsigned(message);
    transaction.sign(&[&keypair], blockhash);

    let signature = transaction
        .signatures
        .first()
        .ok_or_else(|| "Solana transaction missing signature".to_string())?
        .to_string();
    let raw_tx = bincode::serialize(&transaction)
        .map_err(|_| "Failed to serialize Solana transaction".to_string())?;
    let raw_tx_base64 = base64::engine::general_purpose::STANDARD.encode(raw_tx);

    Ok(SignedSolanaTransfer {
        signature,
        raw_tx_base64,
        recent_blockhash: draft.recent_blockhash.to_string(),
        fee_lamports: draft.fee_lamports,
    })
}

pub(crate) async fn sign_solana_transfer(
    mnemonic: &str,
    from: &str,
    to: &str,
    lamports: u64,
) -> Result<SignedSolanaTransfer, String> {
    let recent_blockhash = fetch_latest_solana_blockhash().await?;
    let instructions = native_transfer_instructions(from, to, lamports)?;
    let fee_lamports = estimate_solana_fee(from, instructions, &recent_blockhash).await?;
    sign_solana_transfer_with_blockhash(
        mnemonic,
        from,
        to,
        lamports,
        &recent_blockhash,
        fee_lamports,
    )
}

pub(crate) async fn sign_solana_token_transfer(
    mnemonic: &str,
    from: &str,
    to: &str,
    mint: &str,
    amount: u64,
    decimals: u8,
) -> Result<SignedSolanaTransfer, String> {
    let recent_blockhash = fetch_latest_solana_blockhash().await?;
    let instructions = spl_token_transfer_instructions(from, to, mint, amount, decimals)?;
    let fee_lamports = estimate_solana_fee(from, instructions, &recent_blockhash).await?;

    sign_solana_token_transfer_with_blockhash(
        mnemonic,
        from,
        to,
        mint,
        amount,
        decimals,
        &recent_blockhash,
        fee_lamports,
    )
}

async fn estimate_solana_fee(
    from: &str,
    instructions: Vec<Instruction>,
    recent_blockhash: &str,
) -> Result<u64, String> {
    let from_pubkey = parse_pubkey(from, "from")?;
    let blockhash = parse_blockhash(recent_blockhash)?;
    let message = Message::new_with_blockhash(&instructions, Some(&from_pubkey), &blockhash);
    let message_bytes = bincode::serialize(&message)
        .map_err(|_| "Failed to serialize Solana fee message".to_string())?;
    let message_base64 = base64::engine::general_purpose::STANDARD.encode(message_bytes);
    fetch_solana_fee_for_message(&message_base64).await
}

fn parse_pubkey(value: &str, label: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value).map_err(|_| format!("Invalid Solana {label} address"))
}

fn parse_blockhash(value: &str) -> Result<Hash, String> {
    Hash::from_str(value).map_err(|_| "Invalid Solana recent blockhash".to_string())
}

fn native_transfer_instructions(
    from: &str,
    to: &str,
    lamports: u64,
) -> Result<Vec<Instruction>, String> {
    let from_pubkey = parse_pubkey(from, "from")?;
    let to_pubkey = parse_pubkey(to, "recipient")?;
    Ok(vec![system_instruction::transfer(
        &from_pubkey,
        &to_pubkey,
        lamports,
    )])
}

fn spl_token_transfer_instructions(
    from: &str,
    to: &str,
    mint: &str,
    amount: u64,
    decimals: u8,
) -> Result<Vec<Instruction>, String> {
    let from_pubkey = parse_pubkey(from, "from")?;
    let to_pubkey = parse_pubkey(to, "recipient")?;
    let mint_pubkey = parse_pubkey(mint, "mint")?;
    let source_ata = get_associated_token_address(&from_pubkey, &mint_pubkey);
    let destination_ata = get_associated_token_address(&to_pubkey, &mint_pubkey);
    let create_destination = create_associated_token_account_idempotent(
        &from_pubkey,
        &to_pubkey,
        &mint_pubkey,
        &spl_token::ID,
    );
    let transfer = transfer_checked(
        &spl_token::ID,
        &source_ata,
        &mint_pubkey,
        &destination_ata,
        &from_pubkey,
        &[],
        amount,
        decimals,
    )
    .map_err(|_| "Failed to build SPL token transfer".to_string())?;
    Ok(vec![create_destination, transfer])
}
