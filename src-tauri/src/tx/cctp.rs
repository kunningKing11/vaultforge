//! Operation-specific CCTP calldata, compatible with the shared EIP-1559 draft.

use sha3::{Digest, Keccak256};

use crate::cctp::{CctpTransfer, STANDARD_FINALITY_THRESHOLD};
use crate::tx::evm::{EvmCall, evm_address_word, evm_uint_word};

fn selector(signature: &str) -> Vec<u8> {
    Keccak256::digest(signature.as_bytes())[..4].to_vec()
}

pub(crate) fn build_approval_call(transfer: &CctpTransfer) -> Result<EvmCall, String> {
    transfer.route.validate()?;
    if transfer.route.source.network.kind != "evm" {
        return Err("Solana burns use native program instructions, not ERC-20 approvals".into());
    }
    let mut data = selector("approve(address,uint256)");
    data.extend_from_slice(&evm_address_word(transfer.route.source.token_messenger)?);
    // Approve the requested burn amount, not unlimited spending.
    data.extend_from_slice(&evm_uint_word(transfer.amount()));
    Ok(EvmCall {
        to: transfer.route.source.usdc.into(),
        value: 0,
        data,
    })
}

pub(crate) fn build_burn_call(transfer: &CctpTransfer) -> Result<EvmCall, String> {
    transfer.route.validate()?;
    if transfer.route.source.network.kind != "evm" {
        return Err("Solana burns require a native program builder".into());
    }
    let mut data =
        selector("depositForBurn(uint256,uint32,bytes32,address,bytes32,uint256,uint32)");
    for word in [
        evm_uint_word(transfer.amount()),
        evm_uint_word(transfer.route.destination.domain as u128),
        transfer.mint_recipient_word()?,
        evm_address_word(transfer.route.source.usdc)?,
        transfer.caller_word()?, // source wallet submits the mint
        evm_uint_word(0), // zero-fee-only policy, rechecked against Circle before preparation
        evm_uint_word(STANDARD_FINALITY_THRESHOLD as u128),
    ] {
        data.extend_from_slice(&word);
    }
    Ok(EvmCall {
        to: transfer.route.source.token_messenger.into(),
        value: 0,
        data,
    })
}

/// Internal encoder only: task 9 must bind and validate the attested message before
/// any execution command uses it. This function neither signs nor authorizes mint.
pub(crate) fn build_mint_call(
    transfer: &CctpTransfer,
    message: &[u8],
    attestation: &[u8],
) -> Result<EvmCall, String> {
    transfer.route.validate()?;
    if transfer.route.destination.network.kind != "evm" {
        return Err("Solana mint preflight uses the native attester verifier".into());
    }
    if message.is_empty()
        || attestation.is_empty()
        || message.len() > 16_384
        || attestation.len() > 16_384
    {
        return Err(
            "CCTP message and attestation must be nonempty and at most 16384 bytes each".into(),
        );
    }
    let message_size = message.len().div_ceil(32) * 32;
    let mut data = selector("receiveMessage(bytes,bytes)");
    data.extend_from_slice(&evm_uint_word(64));
    data.extend_from_slice(&evm_uint_word((96 + message_size) as u128));
    for bytes in [message, attestation] {
        data.extend_from_slice(&evm_uint_word(bytes.len() as u128));
        data.extend_from_slice(bytes);
        data.resize(data.len() + (32 - bytes.len() % 32) % 32, 0);
    }
    Ok(EvmCall {
        to: transfer.route.destination.message_transmitter.into(),
        value: 0,
        data,
    })
}

#[cfg(test)]
#[path = "../tests/tx/cctp.rs"]
mod tests;
