use std::str::FromStr;

use base64::Engine;
use solana_hash::Hash;
use solana_instruction::Instruction;
use solana_keypair::{Keypair, keypair_from_seed};
use solana_message::Message;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::instruction as system_instruction;
use solana_transaction::Transaction;
use spl_associated_token_account_interface::{
    address::get_associated_token_address, instruction::create_associated_token_account_idempotent,
};
use spl_token_interface::instruction::transfer_checked;

use crate::derivation::solana_secret_key_from_mnemonic;
use crate::providers::solana::{
    SolanaTokenAccount, fetch_latest_solana_blockhash, fetch_solana_fee_for_message,
};

const MAX_TRANSACTION_BYTES: usize = 1232;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SolanaTokenSource {
    pub(crate) address: String,
    pub(crate) amount: u64,
}

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

pub(crate) struct SolanaTokenTransferDraft<'a> {
    pub(crate) mnemonic: &'a str,
    pub(crate) from: &'a str,
    pub(crate) to: &'a str,
    pub(crate) mint: &'a str,
    pub(crate) sources: &'a [SolanaTokenSource],
    pub(crate) decimals: u8,
    pub(crate) recent_blockhash: &'a str,
    pub(crate) fee_lamports: u64,
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
    draft: SolanaTokenTransferDraft<'_>,
) -> Result<SignedSolanaTransfer, String> {
    let instructions = spl_token_transfer_instructions(
        draft.from,
        draft.to,
        draft.mint,
        draft.sources,
        draft.decimals,
    )?;

    sign_solana_instructions(SolanaTransferDraft {
        mnemonic: draft.mnemonic,
        from: draft.from,
        recent_blockhash: draft.recent_blockhash,
        fee_lamports: draft.fee_lamports,
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
    let raw_tx = wincode::serialize(&transaction)
        .map_err(|_| "Failed to serialize Solana transaction".to_string())?;
    if raw_tx.len() > MAX_TRANSACTION_BYTES {
        return Err("Solana transfer uses too many token accounts; consolidate the token accounts before sending".to_string());
    }
    let raw_tx_base64 = base64::engine::general_purpose::STANDARD.encode(raw_tx);

    Ok(SignedSolanaTransfer {
        signature,
        raw_tx_base64,
        recent_blockhash: draft.recent_blockhash.to_string(),
        fee_lamports: draft.fee_lamports,
    })
}

pub(crate) async fn sign_solana_transfer(
    client: &reqwest::Client,
    mnemonic: &str,
    from: &str,
    to: &str,
    lamports: u64,
) -> Result<SignedSolanaTransfer, String> {
    let recent_blockhash = fetch_latest_solana_blockhash(client).await?;
    let instructions = native_transfer_instructions(from, to, lamports)?;
    let fee_lamports = estimate_solana_fee(client, from, instructions, &recent_blockhash).await?;
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
    client: &reqwest::Client,
    mnemonic: &str,
    from: &str,
    to: &str,
    mint: &str,
    sources: &[SolanaTokenSource],
    decimals: u8,
) -> Result<SignedSolanaTransfer, String> {
    let recent_blockhash = fetch_latest_solana_blockhash(client).await?;
    let instructions = spl_token_transfer_instructions(from, to, mint, sources, decimals)?;
    let fee_lamports = estimate_solana_fee(client, from, instructions, &recent_blockhash).await?;

    sign_solana_token_transfer_with_blockhash(SolanaTokenTransferDraft {
        mnemonic,
        from,
        to,
        mint,
        sources,
        decimals,
        recent_blockhash: &recent_blockhash,
        fee_lamports,
    })
}

pub(crate) fn select_solana_token_sources(
    wallet_address: &str,
    mint: &str,
    decimals: u8,
    amount: u64,
    accounts: &[SolanaTokenAccount],
) -> Result<(Vec<SolanaTokenSource>, u128), String> {
    let ata = solana_associated_token_address(wallet_address, mint)?;
    let mut candidates: Vec<&SolanaTokenAccount> = accounts
        .iter()
        .filter(|account| {
            account.mint == mint && account.decimals == decimals && account.amount > 0
        })
        .collect();
    candidates.sort_by(|left, right| {
        (
            left.address != ata,
            std::cmp::Reverse(left.amount),
            &left.address,
        )
            .cmp(&(
                right.address != ata,
                std::cmp::Reverse(right.amount),
                &right.address,
            ))
    });

    let total_balance = candidates
        .iter()
        .fold(0u128, |total, account| total + u128::from(account.amount));
    let mut remaining = amount;
    let mut sources = Vec::new();
    for account in candidates {
        let spend = account.amount.min(remaining);
        if spend > 0 {
            sources.push(SolanaTokenSource {
                address: account.address.clone(),
                amount: spend,
            });
            remaining -= spend;
        }
        if remaining == 0 {
            return Ok((sources, total_balance));
        }
    }

    Err("Insufficient live SPL token balance across wallet token accounts".to_string())
}

async fn estimate_solana_fee(
    client: &reqwest::Client,
    from: &str,
    instructions: Vec<Instruction>,
    recent_blockhash: &str,
) -> Result<u64, String> {
    let from_pubkey = parse_pubkey(from, "from")?;
    let blockhash = parse_blockhash(recent_blockhash)?;
    let message = Message::new_with_blockhash(&instructions, Some(&from_pubkey), &blockhash);
    let message_bytes = wincode::serialize(&message)
        .map_err(|_| "Failed to serialize Solana fee message".to_string())?;
    let message_base64 = base64::engine::general_purpose::STANDARD.encode(message_bytes);
    fetch_solana_fee_for_message(client, &message_base64).await
}

fn parse_pubkey(value: &str, label: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value).map_err(|_| format!("Invalid Solana {label} address"))
}

fn parse_blockhash(value: &str) -> Result<Hash, String> {
    Hash::from_str(value).map_err(|_| "Invalid Solana recent blockhash".to_string())
}

pub(crate) fn solana_associated_token_address(owner: &str, mint: &str) -> Result<String, String> {
    let owner_pubkey = parse_pubkey(owner, "owner")?;
    let mint_pubkey = parse_pubkey(mint, "mint")?;
    Ok(get_associated_token_address(&owner_pubkey, &mint_pubkey).to_string())
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
    sources: &[SolanaTokenSource],
    decimals: u8,
) -> Result<Vec<Instruction>, String> {
    let from_pubkey = parse_pubkey(from, "from")?;
    let to_pubkey = parse_pubkey(to, "recipient")?;
    let mint_pubkey = parse_pubkey(mint, "mint")?;
    let destination_ata = get_associated_token_address(&to_pubkey, &mint_pubkey);
    let create_destination = create_associated_token_account_idempotent(
        &from_pubkey,
        &to_pubkey,
        &mint_pubkey,
        &spl_token_interface::ID,
    );
    if sources.is_empty() {
        return Err("Solana token transfer has no source accounts".to_string());
    }
    let mut instructions = vec![create_destination];
    for source in sources {
        let source_pubkey = parse_pubkey(&source.address, "token source")?;
        instructions.push(
            transfer_checked(
                &spl_token_interface::ID,
                &source_pubkey,
                &mint_pubkey,
                &destination_ata,
                &from_pubkey,
                &[],
                source.amount,
                decimals,
            )
            .map_err(|_| "Failed to build SPL token transfer".to_string())?,
        );
    }
    Ok(instructions)
}

#[cfg(test)]
#[path = "../tests/tx/solana.rs"]
mod tests;
