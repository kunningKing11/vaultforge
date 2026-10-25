//! Transaction validation for direct Jupiter Metis orders.
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use solana_message::VersionedMessage;
use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;

use crate::operations::SolanaLookup;
use crate::providers::jupiter::{JupiterOrder, OrderRequest};
use crate::tx::swap_solana::{self as encoding, key};

pub(crate) struct ValidatedJupiterOrder {
    pub(crate) transaction: VersionedTransaction,
    pub(crate) wallet_signer_index: usize,
    pub(crate) message_hash: String,
    pub(crate) fee_token_accounts: Vec<(String, String)>,
}

/// Accept reviewed Metis fees while preserving the total input and net minimum.
pub(crate) fn validate_order(
    order: &JupiterOrder,
    request: &OrderRequest,
    verified_lookups: &[SolanaLookup],
) -> Result<ValidatedJupiterOrder, String> {
    if order.router != "metis" || order.raw_response["gasless"].as_bool() != Some(false) {
        return Err("Jupiter engine or sponsorship is not supported".into());
    }
    let amount: u64 = request
        .amount_base_units
        .parse()
        .map_err(|_| "Invalid Jupiter amount")?;
    let output: u64 = order
        .quoted_output_base_units
        .parse()
        .map_err(|_| "Invalid Jupiter output")?;
    let minimum: u64 = order
        .minimum_output_base_units
        .parse()
        .map_err(|_| "Invalid Jupiter minimum")?;
    if amount == 0
        || minimum == 0
        || minimum > output
        || u128::from(minimum)
            < crate::providers::uniswap::minimum_output(u128::from(output), request.slippage_bps)?
        || crate::providers::jupiter::normalize_mint(&request.input_mint)
            == crate::providers::jupiter::normalize_mint(&request.output_mint)
    {
        return Err("Jupiter amount or slippage terms changed".into());
    }
    let transaction = encoding::decode(&order.transaction_base64)?;
    if !matches!(transaction.message, VersionedMessage::V0(_))
        || transaction.signatures.len() != 1
        || transaction
            .signatures
            .iter()
            .any(|s| *s != Default::default())
    {
        return Err("Jupiter requires an unsigned v0 order with the wallet as sole signer".into());
    }
    let fee = platform_fee(order, request, amount, output)?;
    let (validation_view, trade_amount, trade_minimum, fee_token_accounts) = fee_validation_view(
        &transaction,
        verified_lookups,
        request,
        &fee,
        amount,
        minimum,
    )?;
    encoding::validate(
        &STANDARD.encode(
            wincode::serialize(&validation_view)
                .map_err(|_| "Jupiter validation encoding failed")?,
        ),
        verified_lookups,
        &request.taker,
        &request.input_mint,
        &request.output_mint,
        trade_amount,
        trade_minimum,
        request.slippage_bps,
    )?;
    let keys = encoding::resolved_keys(&transaction, verified_lookups)?;
    for instruction in validation_view.message.instructions() {
        let program = keys
            .get(usize::from(instruction.program_id_index))
            .ok_or("Invalid Jupiter program")?
            .to_string();
        // Direct orders do not inherit LI.FI fee transfers or generic guard exceptions.
        match program.as_str() {
            encoding::JUPITER | "ComputeBudget111111111111111111111111111111" => {}
            "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL" => {
                let owner = instruction
                    .accounts
                    .get(2)
                    .and_then(|i| keys.get(usize::from(*i)));
                let mint = instruction
                    .accounts
                    .get(3)
                    .and_then(|i| keys.get(usize::from(*i)));
                if owner != Some(&key(&request.taker)?)
                    || ![
                        key(encoding::mint(&request.input_mint))?,
                        key(encoding::mint(&request.output_mint))?,
                    ]
                    .iter()
                    .any(|k| Some(k) == mint)
                {
                    return Err("Jupiter contains unreviewed account rent".into());
                }
            }
            encoding::NATIVE => {
                let source = instruction
                    .accounts
                    .first()
                    .and_then(|i| keys.get(usize::from(*i)));
                let destination = instruction
                    .accounts
                    .get(1)
                    .and_then(|i| keys.get(usize::from(*i)));
                if instruction.accounts.len() != 2
                    || instruction.data.len() != 12
                    || instruction.data[..4] != [2, 0, 0, 0]
                    || source != Some(&key(&request.taker)?)
                    || request.input_mint != encoding::NATIVE
                    || destination != Some(&encoding::associated(&request.taker, encoding::NATIVE)?)
                {
                    return Err("Jupiter contains an extra native transfer".into());
                }
            }
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA" => {
                let wsol_ata = encoding::associated(&request.taker, encoding::NATIVE)?;
                let account = |position: usize| {
                    instruction
                        .accounts
                        .get(position)
                        .and_then(|i| keys.get(usize::from(*i)))
                };
                let valid = match instruction.data.as_slice() {
                    [17] => instruction.accounts.len() == 1 && account(0) == Some(&wsol_ata),
                    [9] => {
                        instruction.accounts.len() == 3
                            && account(0) == Some(&wsol_ata)
                            && account(1) == Some(&key(&request.taker)?)
                            && account(2) == Some(&key(&request.taker)?)
                    }
                    _ => false,
                };
                if !valid {
                    return Err("Jupiter contains an extra token fee transfer".into());
                }
            }
            _ => return Err(format!("Unreviewed direct Jupiter program {program}")),
        }
    }
    let message_hash = hex::encode(Sha256::digest(transaction.message.serialize()));
    Ok(ValidatedJupiterOrder {
        transaction,
        wallet_signer_index: 0,
        message_hash,
        fee_token_accounts,
    })
}

#[derive(Default)]
struct PlatformFee {
    bps: u16,
    amount: u64,
    mint: String,
    from_input: bool,
}

fn platform_fee(
    order: &JupiterOrder,
    request: &OrderRequest,
    input: u64,
    output: u64,
) -> Result<PlatformFee, String> {
    let raw = &order.raw_response;
    let bps = raw["feeBps"]
        .as_u64()
        .filter(|bps| *bps < 10_000)
        .ok_or("Invalid Jupiter fee rate")? as u16;
    let platform = raw.get("platformFee").filter(|v| !v.is_null());
    if bps == 0 {
        if platform.is_some_and(|v| {
            v["amount"].as_str() != Some("0")
                || v.get("feeBps").is_some_and(|rate| rate.as_u64() != Some(0))
        }) {
            return Err("Jupiter platform fee metadata disagrees with its total fee".into());
        }
        return Ok(PlatformFee::default());
    }
    let platform = platform.ok_or("Jupiter platform fee details missing")?;
    if platform["feeBps"].as_u64() != Some(u64::from(bps))
        || raw.get("referralAccount").is_some_and(|v| !v.is_null())
    {
        return Err("Jupiter fee includes an unrequested referral or sponsorship charge".into());
    }
    let mint = raw["feeMint"].as_str().ok_or("Jupiter fee mint missing")?;
    if platform["feeMint"].as_str() != Some(mint) {
        return Err("Jupiter platform fee mint changed".into());
    }
    let from_input = mint == crate::providers::jupiter::normalize_mint(&request.input_mint);
    if !from_input && mint != crate::providers::jupiter::normalize_mint(&request.output_mint) {
        return Err("Jupiter fee uses an unrelated asset".into());
    }
    let text = platform["amount"]
        .as_str()
        .ok_or("Jupiter fee amount missing")?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("Invalid Jupiter fee amount".into());
    }
    let amount = text
        .parse::<u64>()
        .map_err(|_| "Jupiter fee amount overflows")?;
    let basis = if from_input {
        u128::from(input)
    } else {
        u128::from(output) + u128::from(amount)
    };
    let scaled = basis * u128::from(bps);
    // Providers may round the quoted fee to either adjacent base unit.
    if u128::from(amount) < scaled / 10_000
        || u128::from(amount) > scaled.div_ceil(10_000)
        || (from_input && amount >= input)
    {
        return Err("Jupiter fee amount disagrees with its quoted rate".into());
    }
    Ok(PlatformFee {
        bps,
        amount,
        mint: mint.into(),
        from_input,
    })
}

fn integer(data: &[u8], offset: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(
        data.get(offset..offset + 8)
            .ok_or("Truncated Jupiter fee instruction")?
            .try_into()
            .map_err(|_| "Invalid Jupiter fee amount")?,
    ))
}

/// Remove only independently checked fee collection from a validation copy.
/// The original message is returned for signing; this copy is never executed.
fn fee_validation_view(
    original: &VersionedTransaction,
    lookups: &[SolanaLookup],
    request: &OrderRequest,
    fee: &PlatformFee,
    amount: u64,
    minimum: u64,
) -> Result<(VersionedTransaction, u128, u128, Vec<(String, String)>), String> {
    let keys = encoding::resolved_keys(original, lookups)?;
    let sender = key(&request.taker)?;
    let fee_source = encoding::associated(&request.taker, &fee.mint).ok();
    let input_ata = encoding::associated(&request.taker, &request.input_mint)?;
    let output_ata = encoding::associated(&request.taker, &request.output_mint)?;
    let mut view = original.clone();
    let VersionedMessage::V0(message) = &mut view.message else {
        return Err("Jupiter v0 required".into());
    };
    let mut removed = Vec::new();
    let mut accounts = Vec::new();
    let mut embedded = false;
    let mut swap_seen = false;
    let mut wrapped_fee = false;
    for (index, instruction) in message.instructions.iter_mut().enumerate() {
        let account = |position: usize| -> Result<Pubkey, String> {
            instruction
                .accounts
                .get(position)
                .and_then(|i| keys.get(usize::from(*i)))
                .copied()
                .ok_or("Jupiter fee account missing".into())
        };
        let program = keys
            .get(usize::from(instruction.program_id_index))
            .ok_or("Jupiter program missing")?
            .to_string();
        let data = &instruction.data;
        if program == encoding::JUPITER {
            swap_seen = true;
            let shared =
                data.get(..8) == Some(&Sha256::digest(b"global:shared_accounts_route")[..8]);
            let route = data.get(..8) == Some(&Sha256::digest(b"global:route")[..8]);
            if shared || route {
                let offset = data.len().checked_sub(1).ok_or("Jupiter route truncated")?;
                let encoded_bps = u16::from(data[offset]);
                if encoded_bps != 0 {
                    if encoded_bps != fee.bps || embedded || !removed.is_empty() {
                        return Err("Jupiter encoded platform fee differs from its quote".into());
                    }
                    let collector = account(if shared { 9 } else { 6 })?;
                    if [sender, input_ata, output_ata, key(encoding::JUPITER)?].contains(&collector)
                    {
                        return Err("Invalid Jupiter fee collector".into());
                    }
                    accounts.push((collector.to_string(), fee.mint.clone()));
                    embedded = true;
                    instruction.data[offset] = 0;
                }
            }
            continue;
        }
        if fee.amount == 0 {
            continue;
        }
        let (source, collector, authority, transferred, token_transfer) = if program
            == encoding::NATIVE
            && data.len() == 12
            && data[..4] == [2, 0, 0, 0]
            && fee.mint == encoding::WSOL
            && account(0)? == sender
            && account(1)? != input_ata
        {
            (sender, account(1)?, sender, integer(data, 4)?, false)
        } else if program == "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA" {
            match data.as_slice() {
                [3, ..] if data.len() == 9 => (
                    account(0)?,
                    account(1)?,
                    account(2)?,
                    integer(data, 1)?,
                    true,
                ),
                [12, ..] if data.len() == 10 && account(1)? == key(&fee.mint)? => (
                    account(0)?,
                    account(2)?,
                    account(3)?,
                    integer(data, 1)?,
                    true,
                ),
                _ => continue,
            }
        } else {
            continue;
        };
        if authority != sender
            || transferred != fee.amount
            || (token_transfer && Some(source) != fee_source)
            || [sender, input_ata, output_ata].contains(&collector)
            || fee.from_input == swap_seen
            || embedded
            || !removed.is_empty()
        {
            return Err(
                "Jupiter fee transfer differs from its reviewed amount, asset, or ordering".into(),
            );
        }
        if token_transfer {
            accounts.push((collector.to_string(), fee.mint.clone()));
            wrapped_fee = fee.from_input && request.input_mint == encoding::NATIVE;
        } else if (fee.from_input && request.input_mint != encoding::NATIVE)
            || (!fee.from_input && request.output_mint != encoding::NATIVE)
        {
            return Err("Jupiter native fee does not match the selected native asset".into());
        }
        removed.push(index);
    }
    if fee.amount > 0 && !embedded && removed.is_empty() {
        return Err("Jupiter platform fee has no matching collection instruction".into());
    }
    let mut trade_amount = u128::from(amount);
    let mut trade_minimum = u128::from(minimum);
    if !removed.is_empty() {
        if fee.from_input {
            trade_amount = trade_amount
                .checked_sub(u128::from(fee.amount))
                .ok_or("Jupiter fee exceeds input")?;
            if wrapped_fee {
                let mut wraps = 0;
                for instruction in &mut message.instructions {
                    if keys.get(usize::from(instruction.program_id_index))
                        == Some(&key(encoding::NATIVE)?)
                        && instruction
                            .accounts
                            .get(1)
                            .and_then(|i| keys.get(usize::from(*i)))
                            == Some(&input_ata)
                    {
                        let wrapped = integer(&instruction.data, 4)?
                            .checked_sub(fee.amount)
                            .ok_or("Jupiter wrap cannot cover fee")?;
                        instruction.data[4..12].copy_from_slice(&wrapped.to_le_bytes());
                        wraps += 1;
                    }
                }
                if wraps != 1 {
                    return Err("Jupiter fee requires one reviewed SOL wrap".into());
                }
            }
        } else {
            trade_minimum += u128::from(fee.amount);
        }
    } else if embedded && !fee.from_input {
        trade_minimum = (trade_minimum * 10_000).div_ceil(10_000 - u128::from(fee.bps));
    }
    for index in removed.into_iter().rev() {
        message.instructions.remove(index);
    }
    Ok((view, trade_amount, trade_minimum, accounts))
}

/// Verify message equality and the sole wallet signature before execution.
pub(crate) fn verify_signed_order(
    order: &JupiterOrder,
    signed_transaction_base64: &str,
    taker: &str,
) -> Result<(), String> {
    let original = encoding::decode(&order.transaction_base64)?;
    let signed = encoding::decode(signed_transaction_base64)?;
    if original.message != signed.message
        || signed.message.header().num_required_signatures != 1
        || signed.message.static_account_keys().first() != Some(&key(taker)?)
        || signed.signatures.len() != 1
        || !signed.signatures[0].verify(key(taker)?.as_ref(), &signed.message.serialize())
    {
        return Err(
            "Signed Jupiter transaction differs from the reviewed message or signer".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/tx/jupiter.rs"]
pub(crate) mod tests;
