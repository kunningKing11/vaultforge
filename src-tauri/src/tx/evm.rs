use k256::ecdsa::signature::hazmat::PrehashSigner;
use sha3::{Digest, Keccak256};

use crate::address::evm::validate_address;

#[derive(Debug)]
pub(crate) struct EvmCall {
    pub(crate) to: String,
    pub(crate) value: u128,
    pub(crate) data: Vec<u8>,
}

pub(crate) struct Eip1559TxDraft<'a> {
    pub(crate) signing_key: &'a k256::ecdsa::SigningKey,
    pub(crate) chain_id: u64,
    pub(crate) nonce: u64,
    pub(crate) max_priority_fee_per_gas: u128,
    pub(crate) max_fee_per_gas: u128,
    pub(crate) gas_limit: u64,
    pub(crate) to: &'a str,
    pub(crate) value: u128,
    pub(crate) data: &'a [u8],
}

pub(crate) fn build_native_transfer_call(recipient: &str, amount: u128) -> Result<EvmCall, String> {
    validate_address(recipient)?;

    Ok(EvmCall {
        to: recipient.to_string(),
        value: amount,
        data: Vec::new(),
    })
}

pub(crate) fn build_erc20_transfer_call(
    contract: &str,
    recipient: &str,
    amount: u128,
) -> Result<EvmCall, String> {
    validate_address(contract)?;
    validate_address(recipient)?;

    Ok(EvmCall {
        to: contract.to_string(),
        value: 0,
        data: encode_erc20_transfer(recipient, amount)?,
    })
}

/// ABI address encoding shared by transfer, allowance, and CCTP builders.
pub(crate) fn evm_address_word(address: &str) -> Result<[u8; 32], String> {
    validate_address(address)?;
    let bytes = hex::decode(&address[2..]).map_err(|_| "Invalid EVM address")?;
    let mut word = [0; 32];
    word[12..].copy_from_slice(&bytes);
    Ok(word)
}

pub(crate) fn evm_uint_word(value: u128) -> [u8; 32] {
    let mut word = [0; 32];
    word[16..].copy_from_slice(&value.to_be_bytes());
    word
}

pub(crate) fn encode_erc20_transfer(recipient: &str, amount: u128) -> Result<Vec<u8>, String> {
    let mut data = vec![0xa9, 0x05, 0x9c, 0xbb];
    data.extend_from_slice(&evm_address_word(recipient)?);
    data.extend_from_slice(&evm_uint_word(amount));
    Ok(data)
}

fn trim_leading_zeroes(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|&byte| byte != 0)
        .unwrap_or(bytes.len());
    &bytes[start..]
}

pub(crate) fn sign_eip1559_transfer(
    draft: &Eip1559TxDraft,
) -> Result<(Vec<u8>, String, String, String, String), String> {
    let to_bytes = hex::decode(draft.to.trim_start_matches("0x"))
        .map_err(|_| "Invalid to address".to_string())?;

    let mut stream = rlp::RlpStream::new();
    stream.begin_list(9);
    stream.append(&draft.chain_id);
    stream.append(&draft.nonce);
    stream.append(&draft.max_priority_fee_per_gas);
    stream.append(&draft.max_fee_per_gas);
    stream.append(&draft.gas_limit);
    stream.append(&to_bytes);
    stream.append(&draft.value);
    stream.append(&draft.data.to_vec());
    stream.begin_list(0);

    let unsigned_data = stream.out().to_vec();

    let mut sig_hash_input = vec![0x02u8];
    sig_hash_input.extend_from_slice(&unsigned_data);
    let sig_hash = Keccak256::digest(&sig_hash_input);

    let signature: k256::ecdsa::Signature = draft
        .signing_key
        .sign_prehash(&sig_hash)
        .map_err(|_| "Transaction signing failed".to_string())?;

    let sig_bytes = signature.to_bytes();
    let r_bytes = &sig_bytes[..32];
    let s_bytes = &sig_bytes[32..];
    let r_integer = trim_leading_zeroes(r_bytes);
    let s_integer = trim_leading_zeroes(s_bytes);

    let mut y_parity: u64 = 0;
    let verifying_key = draft.signing_key.verifying_key();
    for is_odd in [false, true] {
        let rid = k256::ecdsa::RecoveryId::new(is_odd, false);
        if let Ok(recovered) =
            k256::ecdsa::VerifyingKey::recover_from_prehash(&sig_hash, &signature, rid)
            && &recovered == verifying_key
        {
            y_parity = if is_odd { 1 } else { 0 };
            break;
        }
    }

    let mut tx_stream = rlp::RlpStream::new();
    tx_stream.begin_list(12);
    tx_stream.append(&draft.chain_id);
    tx_stream.append(&draft.nonce);
    tx_stream.append(&draft.max_priority_fee_per_gas);
    tx_stream.append(&draft.max_fee_per_gas);
    tx_stream.append(&draft.gas_limit);
    tx_stream.append(&to_bytes);
    tx_stream.append(&draft.value);
    tx_stream.append(&draft.data.to_vec());
    tx_stream.begin_list(0);
    tx_stream.append(&y_parity);
    tx_stream.append(&r_integer);
    tx_stream.append(&s_integer);

    let mut signed_data = vec![0x02u8];
    signed_data.extend_from_slice(&tx_stream.out());

    let tx_hash = format!("0x{}", hex::encode(Keccak256::digest(&signed_data)));
    let raw_tx_hex = format!("0x{}", hex::encode(&signed_data));
    let r_hex = hex::encode(r_bytes);
    let s_hex = hex::encode(s_bytes);

    Ok((signed_data, tx_hash, raw_tx_hex, r_hex, s_hex))
}

#[cfg(test)]
#[path = "../tests/tx/evm.rs"]
mod tests;

/// Binds encrypted signed bytes to the expected call, chain, and recovered sender.
/// Shared by contract operations; frontend callers cannot supply signed payloads.
pub(crate) fn validate_signed_call(
    attempt: &crate::operations::TransactionAttempt,
    call: &EvmCall,
    expected_chain: u64,
    expected_sender: &str,
) -> Result<(), String> {
    let context = attempt
        .evm_context
        .as_ref()
        .ok_or("Signed EVM context missing")?;
    let raw = hex::decode(
        attempt
            .raw_tx
            .as_deref()
            .ok_or("Signed bytes missing")?
            .strip_prefix("0x")
            .ok_or("Invalid signed bytes")?,
    )
    .map_err(|_| "Invalid signed bytes")?;
    if raw.first() != Some(&2)
        || format!("0x{}", hex::encode(Keccak256::digest(&raw))) != attempt.tx_hash
    {
        return Err("Stored transaction hash mismatch".into());
    }
    let tx = rlp::Rlp::new(&raw[1..]);
    let parse_error = |_| "Malformed signed EVM transaction".to_string();
    if tx.item_count().map_err(parse_error)? != 12 || tx.as_raw().len() != raw.len() - 1 {
        return Err("Malformed signed EVM transaction".into());
    }
    if context.chain_id != expected_chain
        || !context.from.eq_ignore_ascii_case(expected_sender)
        || !context.to.eq_ignore_ascii_case(&call.to)
        || context.value != "0"
        || context.data != hex::encode(&call.data)
        || attempt.nonce_or_sequence.as_deref() != Some(&context.nonce.to_string())
        || tx.val_at::<u64>(0).map_err(parse_error)? != context.chain_id
        || tx.val_at::<u64>(1).map_err(parse_error)? != context.nonce
        || tx.val_at::<u128>(2).map_err(parse_error)?.to_string()
            != context.max_priority_fee_per_gas
        || tx.val_at::<u128>(3).map_err(parse_error)?.to_string() != context.max_fee_per_gas
        || tx.val_at::<u64>(4).map_err(parse_error)? != context.gas_limit
        || tx.val_at::<Vec<u8>>(5).map_err(parse_error)?
            != hex::decode(&call.to[2..]).map_err(|_| "Invalid contract")?
        || tx.val_at::<u128>(6).map_err(parse_error)? != 0
        || tx.val_at::<Vec<u8>>(7).map_err(parse_error)? != call.data
    {
        return Err("Stored transaction does not match operation intent".into());
    }
    let mut unsigned = rlp::RlpStream::new_list(9);
    for index in 0..9 {
        unsigned.append_raw(tx.at(index).map_err(parse_error)?.as_raw(), 1);
    }
    let mut payload = vec![2];
    payload.extend_from_slice(&unsigned.out());
    let mut scalars = [[0; 32]; 2];
    for (index, scalar) in scalars.iter_mut().enumerate() {
        let item = tx.at(10 + index).map_err(parse_error)?;
        let bytes = item.data().map_err(parse_error)?;
        if bytes.is_empty() || bytes.len() > 32 || bytes[0] == 0 {
            return Err("Invalid signature scalar".into());
        }
        scalar[32 - bytes.len()..].copy_from_slice(bytes);
    }
    let parity = tx.val_at::<u8>(9).map_err(parse_error)?;
    if parity > 1 {
        return Err("Invalid signature parity".into());
    }
    let signature = k256::ecdsa::Signature::from_scalars(scalars[0], scalars[1])
        .map_err(|_| "Invalid signature")?;
    let public = k256::ecdsa::VerifyingKey::recover_from_prehash(
        &Keccak256::digest(&payload),
        &signature,
        k256::ecdsa::RecoveryId::new(parity == 1, false),
    )
    .map_err(|_| "Invalid signature")?
    .to_sec1_point(false);
    let sender = format!(
        "0x{}",
        hex::encode(&Keccak256::digest(&public.as_bytes()[1..])[12..])
    );
    if !sender.eq_ignore_ascii_case(expected_sender) {
        return Err("Stored signature does not match operation sender".into());
    }
    Ok(())
}
