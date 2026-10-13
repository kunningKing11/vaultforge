//! Circle V2 native instruction layouts and versioned transaction construction.
use std::str::FromStr;

use base64::Engine;
use sha2::{Digest, Sha256};
use solana_hash::Hash;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::{AddressLookupTableAccount, Message, VersionedMessage, v0};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;

use crate::cctp::{CctpTransfer, SOLANA_MESSENGER, SOLANA_TRANSMITTER};

pub(crate) fn key(value: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value).map_err(|_| "Invalid CCTP Solana public key".into())
}
pub(crate) fn discriminator(name: &str) -> [u8; 8] {
    Sha256::digest(name.as_bytes())[..8]
        .try_into()
        .expect("digest length")
}
pub(crate) fn pda(program: &Pubkey, seeds: &[&[u8]]) -> Pubkey {
    Pubkey::find_program_address(seeds, program).0
}
fn read(pubkey: Pubkey) -> AccountMeta {
    AccountMeta::new_readonly(pubkey, false)
}
fn write(pubkey: Pubkey) -> AccountMeta {
    AccountMeta::new(pubkey, false)
}
pub(crate) fn used_nonce(message: &[u8]) -> Result<Pubkey, String> {
    Ok(pda(
        &key(SOLANA_TRANSMITTER)?,
        &[
            b"used_nonce",
            message.get(12..44).ok_or("Message nonce missing")?,
        ],
    ))
}

pub(crate) fn burn_instruction(
    transfer: &CctpTransfer,
    event: Pubkey,
) -> Result<Instruction, String> {
    if transfer.route.source.network.kind != "svm" {
        return Err("Native burn requires Solana source".into());
    }
    let messenger = key(SOLANA_MESSENGER)?;
    let transmitter = key(SOLANA_TRANSMITTER)?;
    let owner = key(transfer.sender())?;
    let mint = key(transfer.route.source.usdc)?;
    let domain = transfer.route.destination.domain.to_string();
    let source = spl_associated_token_account_interface::address::get_associated_token_address(
        &owner, &mint,
    );
    let mut data = discriminator("global:deposit_for_burn").to_vec();
    data.extend_from_slice(
        &u64::try_from(transfer.amount())
            .map_err(|_| "Burn amount overflows u64")?
            .to_le_bytes(),
    );
    data.extend_from_slice(&transfer.route.destination.domain.to_le_bytes());
    data.extend_from_slice(&transfer.mint_recipient_word()?);
    data.extend_from_slice(&transfer.caller_word()?);
    data.extend_from_slice(&0u64.to_le_bytes());
    data.extend_from_slice(&2000u32.to_le_bytes());
    Ok(Instruction {
        program_id: messenger,
        accounts: vec![
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new(owner, true),
            read(pda(&messenger, &[b"sender_authority"])),
            write(source),
            read(pda(&messenger, &[b"denylist_account", owner.as_ref()])),
            write(pda(&transmitter, &[b"message_transmitter"])),
            read(pda(&messenger, &[b"token_messenger"])),
            read(pda(
                &messenger,
                &[b"remote_token_messenger", domain.as_bytes()],
            )),
            read(pda(&messenger, &[b"token_minter"])),
            write(pda(&messenger, &[b"local_token", mint.as_ref()])),
            write(mint),
            AccountMeta::new(event, true),
            read(transmitter),
            read(messenger),
            read(spl_token_interface::ID),
            read(solana_system_interface::program::ID),
            read(pda(&messenger, &[b"__event_authority"])),
            read(messenger),
        ],
        data,
    })
}

pub(crate) fn mint_instruction(
    transfer: &CctpTransfer,
    payer: Pubkey,
    fee_recipient: Pubkey,
    message: &[u8],
    attestation: &[u8],
) -> Result<Instruction, String> {
    if transfer.route.destination.network.kind != "svm" {
        return Err("Native mint requires Solana destination".into());
    }
    crate::cctp_attestation::validate_message_intent(transfer, message)?;
    if attestation.is_empty() || attestation.len() % 65 != 0 || attestation.len() > 16384 {
        return Err("Invalid mint attestation size".into());
    }
    let messenger = key(SOLANA_MESSENGER)?;
    let transmitter = key(SOLANA_TRANSMITTER)?;
    let mint = key(transfer.route.destination.usdc)?;
    let domain = transfer.route.source.domain.to_string();
    let mut data = discriminator("global:receive_message").to_vec();
    for bytes in [message, attestation] {
        data.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        data.extend_from_slice(bytes);
    }
    let fee_ata = spl_associated_token_account_interface::address::get_associated_token_address(
        &fee_recipient,
        &mint,
    );
    Ok(Instruction {
        program_id: transmitter,
        accounts: vec![
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(payer, true),
            read(pda(
                &transmitter,
                &[b"message_transmitter_authority", messenger.as_ref()],
            )),
            read(pda(&transmitter, &[b"message_transmitter"])),
            write(used_nonce(message)?),
            read(messenger),
            read(solana_system_interface::program::ID),
            read(pda(&transmitter, &[b"__event_authority"])),
            read(transmitter),
            read(pda(&messenger, &[b"token_messenger"])),
            read(pda(
                &messenger,
                &[b"remote_token_messenger", domain.as_bytes()],
            )),
            read(pda(&messenger, &[b"token_minter"])),
            write(pda(&messenger, &[b"local_token", mint.as_ref()])),
            read(pda(
                &messenger,
                &[b"token_pair", domain.as_bytes(), &message[152..184]],
            )),
            write(fee_ata),
            write(Pubkey::new_from_array(transfer.mint_recipient_word()?)),
            write(pda(&messenger, &[b"custody", mint.as_ref()])),
            read(spl_token_interface::ID),
            read(pda(&messenger, &[b"__event_authority"])),
            read(messenger),
        ],
        data,
    })
}

pub(crate) fn compute_budget() -> Result<Instruction, String> {
    let mut data = vec![2];
    data.extend_from_slice(&500_000u32.to_le_bytes());
    Ok(Instruction {
        program_id: key("ComputeBudget111111111111111111111111111111")?,
        accounts: vec![],
        data,
    })
}

pub(crate) fn provisional_message(transfer: &CctpTransfer) -> Result<Vec<u8>, String> {
    use crate::cctp::address_word;
    use crate::tx::evm::evm_uint_word;
    let mut message = vec![0; 376];
    for (offset, value) in [
        (0, 1),
        (4, transfer.route.source.domain),
        (8, transfer.route.destination.domain),
        (140, 2000),
        (144, 2000),
        (148, 1),
    ] {
        message[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    for (offset, value) in [
        (
            44,
            address_word(
                &transfer.route.source.network,
                transfer.route.source.token_messenger,
            )?,
        ),
        (
            76,
            address_word(
                &transfer.route.destination.network,
                transfer.route.destination.token_messenger,
            )?,
        ),
        (108, transfer.caller_word()?),
        (
            152,
            address_word(&transfer.route.source.network, transfer.route.source.usdc)?,
        ),
        (184, transfer.mint_recipient_word()?),
        (216, evm_uint_word(transfer.amount())),
        (
            248,
            address_word(&transfer.route.source.network, transfer.sender())?,
        ),
    ] {
        message[offset..offset + 32].copy_from_slice(&value);
    }
    Ok(message)
}

pub(crate) fn setup_instructions(
    transfer: &CctpTransfer,
    payer: Pubkey,
    fee_recipient: Pubkey,
    slot: u64,
    create_ata: bool,
) -> Result<(Vec<Instruction>, crate::operations::SolanaLookup), String> {
    use solana_address_lookup_table_interface::instruction::{
        create_lookup_table, extend_lookup_table, freeze_lookup_table,
    };
    let dummy = provisional_message(transfer)?;
    let mint = mint_instruction(transfer, payer, fee_recipient, &dummy, &[0; 130])?;
    let nonce = used_nonce(&dummy)?;
    let mut addresses: Vec<Pubkey> = mint
        .accounts
        .iter()
        .filter(|a| !a.is_signer && a.pubkey != nonce)
        .map(|a| a.pubkey)
        .collect();
    addresses.push(mint.program_id);
    addresses.sort();
    addresses.dedup();
    // The maintained lookup interface currently uses Instruction v4; the wallet
    // message builder uses v3. The account metadata and wire data are identical.
    let (create, table) = create_lookup_table(payer, payer, slot);
    let extend = extend_lookup_table(table, payer, Some(payer), addresses.clone());
    let mut instructions = vec![compute_budget()?];
    if create_ata {
        instructions.push(spl_associated_token_account_interface::instruction::create_associated_token_account_idempotent(&payer,&key(transfer.recipient())?,&key(transfer.route.destination.usdc)?,&spl_token_interface::ID));
    }
    for native in [create, extend, freeze_lookup_table(table, payer)] {
        instructions.push(Instruction {
            program_id: native.program_id,
            accounts: native
                .accounts
                .into_iter()
                .map(|a| AccountMeta {
                    pubkey: a.pubkey,
                    is_signer: a.is_signer,
                    is_writable: a.is_writable,
                })
                .collect(),
            data: native.data,
        });
    }
    Ok((
        instructions,
        crate::operations::SolanaLookup {
            address: table.to_string(),
            addresses: addresses.iter().map(ToString::to_string).collect(),
        },
    ))
}

pub(crate) fn compile(
    payer: Pubkey,
    instructions: &[Instruction],
    blockhash: &str,
    lookup: &[AddressLookupTableAccount],
) -> Result<VersionedMessage, String> {
    let hash = Hash::from_str(blockhash).map_err(|_| "Invalid Solana blockhash")?;
    if lookup.is_empty() {
        Ok(VersionedMessage::Legacy(Message::new_with_blockhash(
            instructions,
            Some(&payer),
            &hash,
        )))
    } else {
        v0::Message::try_compile(&payer, instructions, lookup, hash)
            .map(VersionedMessage::V0)
            .map_err(|_| "Could not compile CCTP lookup-table message".into())
    }
}
pub(crate) fn signed(
    message: VersionedMessage,
    payer: &Keypair,
    event: Option<&Keypair>,
) -> Result<(String, String), String> {
    if message.static_account_keys().first() != Some(&payer.pubkey()) {
        return Err("CCTP fee payer does not match signing key".into());
    }
    let tx = if let Some(event) = event {
        VersionedTransaction::try_new(message, &[payer, event])
    } else {
        VersionedTransaction::try_new(message, &[payer])
    }
    .map_err(|_| "Failed to sign native CCTP transaction")?;
    let bytes = wincode::serialize(&tx).map_err(|_| "Failed to encode native CCTP transaction")?;
    if bytes.len() > 1232 {
        return Err("Native CCTP transaction exceeds 1232-byte packet; a lookup table or separate account setup is required".into());
    }
    Ok((
        tx.signatures
            .first()
            .ok_or("Signature missing")?
            .to_string(),
        base64::engine::general_purpose::STANDARD.encode(bytes),
    ))
}

#[cfg(test)]
#[path = "../tests/tx/cctp_solana.rs"]
mod tests;
