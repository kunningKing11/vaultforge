//! Strict native Solana same-chain swap validation. No EVM address/nonce assumptions.
use crate::operations::SolanaLookup;
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;
use std::str::FromStr;

pub(crate) const NATIVE: &str = "11111111111111111111111111111111";
pub(crate) const WSOL: &str = "So11111111111111111111111111111111111111112";
pub(crate) const JUPITER: &str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";
// Pinned from LI.FI's production Solana fee collection; unknown recipients fail closed.
pub(crate) const LIFI_FEE_OWNER: &str = "34FKjAdVcTax2DHqV2XnbXa9J3zmyKcFuFKWbcmgxjgm";
const TOKEN: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const ATA: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
const COMPUTE: &str = "ComputeBudget111111111111111111111111111111";
pub(crate) fn key(s: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(s).map_err(|_| "Invalid Solana swap address".into())
}
pub(crate) fn mint(s: &str) -> &str {
    if s == NATIVE { WSOL } else { s }
}
pub(crate) fn associated(owner: &str, token: &str) -> Result<Pubkey, String> {
    Ok(
        spl_associated_token_account_interface::address::get_associated_token_address(
            &key(owner)?,
            &key(mint(token))?,
        ),
    )
}
fn u64_at(data: &[u8], start: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(
        data.get(start..start + 8)
            .ok_or("Truncated Solana instruction")?
            .try_into()
            .map_err(|_| "Invalid Solana integer")?,
    ))
}
pub(crate) fn decode(raw: &str) -> Result<VersionedTransaction, String> {
    let bytes = STANDARD
        .decode(raw)
        .map_err(|_| "Invalid Solana swap base64")?;
    if bytes.len() > 1232 {
        return Err("Solana swap exceeds the supported transaction size".into());
    }
    wincode::deserialize(&bytes).map_err(|_| "Unsupported Solana swap transaction encoding".into())
}
pub(crate) fn resolved_keys(
    tx: &VersionedTransaction,
    lookups: &[SolanaLookup],
) -> Result<Vec<Pubkey>, String> {
    let mut keys = tx.message.static_account_keys().to_vec();
    if let Some(tables) = tx.message.address_table_lookups() {
        for writable in [true, false] {
            for table in tables {
                let saved = lookups
                    .iter()
                    .find(|l| l.address == table.account_key.to_string())
                    .ok_or("Solana lookup table was not resolved")?;
                for index in if writable {
                    &table.writable_indexes
                } else {
                    &table.readonly_indexes
                } {
                    keys.push(key(saved
                        .addresses
                        .get(usize::from(*index))
                        .ok_or("Solana lookup index out of range")?)?);
                }
            }
        }
    }
    Ok(keys)
}

/// Only standard wrapping/account setup and one pinned Jupiter exact-input
/// instruction may use the wallet signature. Spending, receiving ATA, minimum,
/// slippage, and authority are bound to the selected trade.
pub(crate) fn validate(
    raw: &str,
    lookups: &[SolanaLookup],
    sender: &str,
    input: &str,
    output: &str,
    amount: u128,
    minimum: u128,
    slippage: u16,
) -> Result<VersionedTransaction, String> {
    let tx = decode(raw)?;
    if tx.message.header().num_required_signatures != 1
        || tx.message.header().num_readonly_signed_accounts != 0
        || tx.signatures.len() != 1
        || tx.message.static_account_keys().first() != Some(&key(sender)?)
    {
        return Err("Solana swap has a foreign payer or additional signer".into());
    }
    let keys = resolved_keys(&tx, lookups)?;
    let input_ata = associated(sender, input)?;
    let output_ata = associated(sender, output)?;
    let sender_key = key(sender)?;
    let instructions = tx.message.instructions();
    if instructions.len() > 16 {
        return Err("Too many Solana swap instructions".into());
    }
    let mut swaps = 0;
    let mut provider_fee = 0u128;
    let mut swapped_input = None;
    let mut wrapped = 0u128;
    let mut unwrapped_output = false;
    for instruction in instructions {
        let program = keys
            .get(usize::from(instruction.program_id_index))
            .ok_or("Invalid Solana program index")?
            .to_string();
        let account = |index: usize| -> Result<Pubkey, String> {
            keys.get(usize::from(
                *instruction
                    .accounts
                    .get(index)
                    .ok_or("Missing Solana instruction account")?,
            ))
            .copied()
            .ok_or("Solana account index out of range".into())
        };
        let data = &instruction.data;
        match program.as_str() {
            COMPUTE => match data.first() {
                Some(2)
                    if data.len() == 5
                        && u32::from_le_bytes(
                            data[1..5].try_into().map_err(|_| "Invalid compute limit")?,
                        ) <= 1_400_000 => {}
                Some(3) if data.len() == 9 => {}
                _ => return Err("Unsupported Solana compute budget instruction".into()),
            },
            "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr" => {
                if !instruction.accounts.is_empty() || data.len() > 128 {
                    return Err("Unsupported Solana swap memo".into());
                }
            }
            NATIVE => {
                if data.len() == 12
                    && data[..4] == [2, 0, 0, 0]
                    && account(0)? == sender_key
                    && u64_at(data, 4)? == 0
                {
                    continue;
                }
                if input == NATIVE
                    && data.len() == 12
                    && data[..4] == [2, 0, 0, 0]
                    && account(0)? == sender_key
                    && account(1)? == key(LIFI_FEE_OWNER)?
                {
                    provider_fee = provider_fee
                        .checked_add(u128::from(u64_at(data, 4)?))
                        .ok_or("Provider fee overflow")?;
                    continue;
                }
                if input != NATIVE
                    || data.len() != 12
                    || data[..4] != [2, 0, 0, 0]
                    || account(0)? != sender_key
                    || account(1)? != input_ata
                {
                    return Err("Solana swap contains an unexpected native transfer".into());
                }
                wrapped = wrapped
                    .checked_add(u128::from(u64_at(data, 4)?))
                    .ok_or("Solana wrap amount overflow")?;
            }
            ATA => {
                let token_mint = account(3)?;
                let owner = account(2)?;
                let allowed_owner = owner == sender_key
                    || (input != NATIVE
                        && owner == key(LIFI_FEE_OWNER)?
                        && token_mint == key(input)?);
                let expected_ata =
                    spl_associated_token_account_interface::address::get_associated_token_address(
                        &owner,
                        &token_mint,
                    );
                if data.as_slice() != [1]
                    || account(0)? != sender_key
                    || !allowed_owner
                    || account(4)? != key(NATIVE)?
                    || account(5)? != key(TOKEN)?
                    || account(1)? != expected_ata
                {
                    return Err("Solana swap contains foreign account creation".into());
                }
            }
            TOKEN => match data.as_slice() {
                [3, ..]
                    if input != NATIVE
                        && data.len() == 9
                        && account(0)? == input_ata
                        && account(1)? == associated(LIFI_FEE_OWNER, input)?
                        && account(2)? == sender_key =>
                {
                    provider_fee = provider_fee
                        .checked_add(u128::from(u64_at(data, 1)?))
                        .ok_or("Provider fee overflow")?;
                }
                [12, ..]
                    if input != NATIVE
                        && data.len() == 10
                        && account(0)? == input_ata
                        && account(1)? == key(input)?
                        && account(2)? == associated(LIFI_FEE_OWNER, input)?
                        && account(3)? == sender_key =>
                {
                    provider_fee = provider_fee
                        .checked_add(u128::from(u64_at(data, 1)?))
                        .ok_or("Provider fee overflow")?;
                }
                [17] if input == NATIVE && account(0)? == input_ata => {}
                [9] if account(1)? == sender_key
                    && account(2)? == sender_key
                    && ((input == NATIVE && account(0)? == input_ata)
                        || (output == NATIVE && account(0)? == output_ata)) =>
                {
                    if output == NATIVE && account(0)? == output_ata {
                        unwrapped_output = true;
                    }
                }
                _ => return Err("Solana swap contains an unexpected token instruction".into()),
            },
            JUPITER => {
                let route = Sha256::digest(b"global:route");
                let shared = Sha256::digest(b"global:shared_accounts_route");
                let shared_route = data.get(..8) == Some(&shared[..8]);
                let route_v2 = data.get(..8) == Some(&Sha256::digest(b"global:route_v2")[..8]);
                if !route_v2 && !shared_route && data.get(..8) != Some(&route[..8]) {
                    return Err("Unsupported Jupiter swap instruction".into());
                }
                let tail = if route_v2 {
                    8
                } else {
                    data.len()
                        .checked_sub(19)
                        .ok_or("Truncated Jupiter route")?
                };
                if route_v2 && (data.len() < 34 || data[26..30] != [0; 4]) {
                    return Err("Unsupported Jupiter platform or positive-slippage fee".into());
                }
                if !route_v2 && tail < if shared_route { 13 } else { 12 } {
                    return Err("Truncated Jupiter route plan".into());
                }
                let encoded_input = u128::from(u64_at(data, tail)?);
                let quoted = u128::from(u64_at(data, tail + 8)?);
                let bps = u16::from_le_bytes(
                    data.get(tail + 16..tail + 18)
                        .ok_or("Jupiter slippage missing")?
                        .try_into()
                        .map_err(|_| "Invalid Jupiter slippage")?,
                );
                let minimum_encoded = quoted
                    .checked_mul(u128::from(
                        10000u16
                            .checked_sub(bps)
                            .ok_or("Invalid Jupiter slippage")?,
                    ))
                    .ok_or("Jupiter minimum overflow")?
                    / 10000;
                let (authority, source, destination, dest_mint) = if shared_route {
                    (2, 3, 6, 8)
                } else {
                    if route_v2 { (0, 1, 2, 4) } else { (1, 2, 3, 5) }
                };
                swapped_input = Some(encoded_input);
                if bps > slippage
                    || minimum_encoded < minimum
                    || minimum == 0
                    || (!route_v2 && data[tail + 18] != 0)
                    || account(authority)? != sender_key
                    || account(source)? != input_ata
                    || account(destination)? != output_ata
                    || (!shared_route
                        && ![output_ata, key(JUPITER)?].contains(&account(if route_v2 {
                            7
                        } else {
                            4
                        })?))
                    || account(dest_mint)? != key(mint(output))?
                    || (shared_route && account(7)? != key(mint(input))?)
                    || (if route_v2 {
                        account(3)? != key(mint(input))?
                            || account(5)? != key(TOKEN)?
                            || account(6)? != key(TOKEN)?
                    } else {
                        account(0)? != key(TOKEN)?
                    })
                {
                    return Err("Jupiter amount, minimum, assets, recipient, or fee changed".into());
                }
                swaps += 1;
            }
            _ => {
                // Provider expiry/price guards cannot change funds without a signer
                // or writable account. Never grant this exception to mutable accounts.
                let static_len = tx.message.static_account_keys().len();
                let writable_loaded: usize = tx
                    .message
                    .address_table_lookups()
                    .map(|tables| tables.iter().map(|t| t.writable_indexes.len()).sum())
                    .unwrap_or(0);
                let readonly_start = static_len
                    .checked_sub(usize::from(
                        tx.message.header().num_readonly_unsigned_accounts,
                    ))
                    .ok_or("Invalid Solana header")?;
                let safe = instruction.accounts.iter().all(|index| {
                    let index = usize::from(*index);
                    index < keys.len()
                        && index >= 1
                        && (if index < static_len {
                            index >= readonly_start
                        } else {
                            index >= static_len + writable_loaded
                        })
                });
                if !safe || data.len() > 128 {
                    return Err(format!("Unsupported Solana swap program {program}"));
                }
            }
        }
    }
    if swaps != 1
        || provider_fee > amount.checked_mul(25).ok_or("Provider fee overflow")? / 10000
        || swapped_input != amount.checked_sub(provider_fee)
        || (input == NATIVE && Some(wrapped) != swapped_input)
        || (input != NATIVE && wrapped != 0)
        || (output == NATIVE && !unwrapped_output)
    {
        return Err("Solana swap wrapping or execution differs from selected trade".into());
    }
    Ok(tx)
}

#[cfg(test)]
#[path = "../tests/tx/swap_solana.rs"]
pub(crate) mod tests;
