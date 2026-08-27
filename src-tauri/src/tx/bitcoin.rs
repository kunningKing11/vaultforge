use bech32::{hrp, segwit};
use k256::ecdsa::signature::hazmat::PrehashSigner;
use ripemd::Ripemd160;
use ripemd::digest::Digest as RipemdDigest;
use sha2::{Digest as Sha2Digest, Sha256};
use std::collections::BTreeMap;
use zeroize::Zeroize;

use crate::derivation::{
    BitcoinAccount, BitcoinDerivedAddress, BitcoinKeyOrigin, secp256k1_private_key_from_mnemonic,
    signing_key_from_private_key,
};
use crate::providers::bitcoin::BitcoinUtxo;

const BITCOIN_P2WPKH_DUST_SATS: u64 = 294;
const BITCOIN_P2WPKH_INPUT_MAX_WITNESS_BYTES: u64 = 109;

#[derive(Clone)]
pub(crate) struct BitcoinTxInput {
    pub(crate) utxo: BitcoinUtxo,
    pub(crate) script_code: Vec<u8>,
    pub(crate) signing_context_index: usize,
}

#[derive(Clone)]
pub(crate) struct BitcoinTxOutput {
    pub(crate) value: u64,
    pub(crate) script_pubkey: Vec<u8>,
}

pub(crate) struct BitcoinSignedTransfer {
    pub(crate) txid: String,
    pub(crate) raw_tx_hex: String,
    pub(crate) first_signature_hex: String,
    pub(crate) fee_sats: u64,
    pub(crate) post_balance: u64,
}

struct BitcoinSigningContext {
    origin: BitcoinKeyOrigin,
    address: String,
    signing_key: k256::ecdsa::SigningKey,
    public_key: Vec<u8>,
    script_code: Vec<u8>,
}

fn bitcoin_varint(value: u64) -> Vec<u8> {
    if value < 0xfd {
        vec![value as u8]
    } else if value <= 0xffff {
        let mut out = vec![0xfd];
        out.extend_from_slice(&(value as u16).to_le_bytes());
        out
    } else if value <= 0xffff_ffff {
        let mut out = vec![0xfe];
        out.extend_from_slice(&(value as u32).to_le_bytes());
        out
    } else {
        let mut out = vec![0xff];
        out.extend_from_slice(&value.to_le_bytes());
        out
    }
}

fn bitcoin_push_data(data: &[u8]) -> Vec<u8> {
    let mut out = bitcoin_varint(data.len() as u64);
    out.extend_from_slice(data);
    out
}

fn bitcoin_p2wpkh_script_pubkey(pubkey_hash: &[u8]) -> Vec<u8> {
    let mut script = vec![0x00, 0x14];
    script.extend_from_slice(pubkey_hash);
    script
}

fn bitcoin_p2pkh_script_code(pubkey_hash: &[u8]) -> Vec<u8> {
    let mut script = vec![0x76, 0xa9, 0x14];
    script.extend_from_slice(pubkey_hash);
    script.extend_from_slice(&[0x88, 0xac]);
    script
}

fn bitcoin_script_pubkey_from_address(address: &str) -> Result<Vec<u8>, String> {
    if address.starts_with("bc1") {
        let (decoded_hrp, version, program) =
            segwit::decode(address).map_err(|_| "Invalid Bitcoin bech32 recipient".to_string())?;
        if decoded_hrp != hrp::BC || version != segwit::VERSION_0 || program.len() != 20 {
            return Err("Unsupported Bitcoin bech32 recipient".to_string());
        }
        return Ok(bitcoin_p2wpkh_script_pubkey(&program));
    }

    let decoded = bs58::decode(address)
        .with_check(None)
        .into_vec()
        .map_err(|_| "Invalid Bitcoin base58 recipient".to_string())?;
    if decoded.len() != 21 {
        return Err("Unsupported Bitcoin base58 recipient length".to_string());
    }
    let version = decoded[0];
    let hash = &decoded[1..];
    match version {
        0x00 => {
            let mut script = vec![0x76, 0xa9, 0x14];
            script.extend_from_slice(hash);
            script.extend_from_slice(&[0x88, 0xac]);
            Ok(script)
        }
        0x05 => {
            let mut script = vec![0xa9, 0x14];
            script.extend_from_slice(hash);
            script.push(0x87);
            Ok(script)
        }
        _ => Err("Only mainnet Bitcoin recipients are supported".to_string()),
    }
}

fn bitcoin_txid_le(txid: &str) -> Result<Vec<u8>, String> {
    let mut bytes = hex::decode(txid).map_err(|_| "Invalid Bitcoin txid hex".to_string())?;
    if bytes.len() != 32 {
        return Err("Bitcoin txid must be 32 bytes".to_string());
    }
    bytes.reverse();
    Ok(bytes)
}

fn bitcoin_double_sha256(data: &[u8]) -> [u8; 32] {
    let first = <Sha256 as Sha2Digest>::digest(data);
    <Sha256 as Sha2Digest>::digest(first).into()
}

fn bitcoin_txid_from_stripped(stripped_tx: &[u8]) -> String {
    let mut hash = bitcoin_double_sha256(stripped_tx);
    hash.reverse();
    hex::encode(hash)
}

fn bitcoin_serialize_outputs(outputs: &[BitcoinTxOutput]) -> Vec<u8> {
    let mut out = bitcoin_varint(outputs.len() as u64);
    for output in outputs {
        out.extend_from_slice(&output.value.to_le_bytes());
        out.extend(bitcoin_push_data(&output.script_pubkey));
    }
    out
}

fn bitcoin_serialize_stripped(
    inputs: &[BitcoinTxInput],
    outputs: &[BitcoinTxOutput],
) -> Result<Vec<u8>, String> {
    let mut tx = vec![];
    tx.extend_from_slice(&2i32.to_le_bytes());
    tx.extend(bitcoin_varint(inputs.len() as u64));
    for input in inputs {
        tx.extend(bitcoin_txid_le(&input.utxo.txid)?);
        tx.extend_from_slice(&input.utxo.vout.to_le_bytes());
        tx.push(0x00);
        tx.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
    }
    tx.extend(bitcoin_serialize_outputs(outputs));
    tx.extend_from_slice(&0u32.to_le_bytes());
    Ok(tx)
}

fn bitcoin_sighash(
    input_index: usize,
    inputs: &[BitcoinTxInput],
    outputs: &[BitcoinTxOutput],
) -> Result<[u8; 32], String> {
    let mut prevouts = vec![];
    let mut sequences = vec![];
    for input in inputs {
        prevouts.extend(bitcoin_txid_le(&input.utxo.txid)?);
        prevouts.extend_from_slice(&input.utxo.vout.to_le_bytes());
        sequences.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
    }

    let hash_prevouts = bitcoin_double_sha256(&prevouts);
    let hash_sequence = bitcoin_double_sha256(&sequences);
    let hash_outputs = bitcoin_double_sha256(&bitcoin_serialize_outputs(outputs));
    let input = inputs
        .get(input_index)
        .ok_or_else(|| "Bitcoin input index out of range".to_string())?;

    let mut preimage = vec![];
    preimage.extend_from_slice(&2i32.to_le_bytes());
    preimage.extend_from_slice(&hash_prevouts);
    preimage.extend_from_slice(&hash_sequence);
    preimage.extend(bitcoin_txid_le(&input.utxo.txid)?);
    preimage.extend_from_slice(&input.utxo.vout.to_le_bytes());
    preimage.extend(bitcoin_push_data(&input.script_code));
    preimage.extend_from_slice(&input.utxo.value.to_le_bytes());
    preimage.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
    preimage.extend_from_slice(&hash_outputs);
    preimage.extend_from_slice(&0u32.to_le_bytes());
    preimage.extend_from_slice(&1u32.to_le_bytes());
    Ok(bitcoin_double_sha256(&preimage))
}

fn bitcoin_varint_len(value: usize) -> u64 {
    match value {
        0..=0xfc => 1,
        0xfd..=0xffff => 3,
        0x1_0000..=0xffff_ffff => 5,
        _ => 9,
    }
}

pub(crate) fn bitcoin_estimated_vbytes(input_count: usize, output_count: usize) -> u64 {
    let input_count = input_count as u64;
    let output_count = output_count as u64;
    let stripped_bytes = 4u64
        .saturating_add(bitcoin_varint_len(input_count as usize))
        .saturating_add(input_count.saturating_mul(41))
        .saturating_add(bitcoin_varint_len(output_count as usize))
        .saturating_add(output_count.saturating_mul(34))
        .saturating_add(4);
    let weight = stripped_bytes
        .saturating_mul(4)
        .saturating_add(2)
        .saturating_add(input_count.saturating_mul(BITCOIN_P2WPKH_INPUT_MAX_WITNESS_BYTES));
    weight.saturating_add(3) / 4
}

pub(crate) fn bitcoin_select_coins(
    utxos: &[BitcoinUtxo],
    amount: u64,
    fee_rate_sat_vb: u64,
) -> Result<(Vec<BitcoinUtxo>, u64, u64), String> {
    let mut selected = vec![];
    let mut total = 0u64;
    let marginal_input_fee = bitcoin_estimated_vbytes(1, 1)
        .saturating_sub(bitcoin_estimated_vbytes(0, 1))
        .checked_mul(fee_rate_sat_vb)
        .ok_or_else(|| "Bitcoin input fee overflowed".to_string())?;
    for utxo in utxos
        .iter()
        .filter(|utxo| utxo.value > marginal_input_fee)
        .filter(|u| u.confirmed)
        .chain(
            utxos
                .iter()
                .filter(|utxo| utxo.value > marginal_input_fee)
                .filter(|u| !u.confirmed),
        )
    {
        selected.push(utxo.clone());
        total = total
            .checked_add(utxo.value)
            .ok_or_else(|| "Bitcoin selected value overflowed".to_string())?;

        let fee_no_change = bitcoin_estimated_vbytes(selected.len(), 1)
            .checked_mul(fee_rate_sat_vb)
            .ok_or_else(|| "Bitcoin transaction fee overflowed".to_string())?;
        let required_no_change = amount
            .checked_add(fee_no_change)
            .ok_or_else(|| "Bitcoin amount plus fee overflowed".to_string())?;
        if total < required_no_change {
            continue;
        }

        let fee_with_change = bitcoin_estimated_vbytes(selected.len(), 2)
            .checked_mul(fee_rate_sat_vb)
            .ok_or_else(|| "Bitcoin transaction fee overflowed".to_string())?;
        let required_with_change = amount
            .checked_add(fee_with_change)
            .and_then(|required| required.checked_add(BITCOIN_P2WPKH_DUST_SATS));
        if required_with_change.is_some_and(|required| total >= required) {
            return Ok((selected, fee_with_change, total - amount - fee_with_change));
        }

        return Ok((selected, total - amount, 0));
    }
    Err("Insufficient BTC balance for amount plus fee".to_string())
}

pub(crate) fn bitcoin_signed_transfer(
    mnemonic: &str,
    from_address: &str,
    to_address: &str,
    amount_sats: u64,
    utxos: &[BitcoinUtxo],
    fee_rate_sat_vb: u64,
    change_address: &BitcoinDerivedAddress,
) -> Result<BitcoinSignedTransfer, String> {
    if amount_sats == 0 {
        return Err("Amount must be greater than zero".to_string());
    }

    let account = BitcoinAccount::from_mnemonic(mnemonic)?;
    if account.primary_address()?.address != from_address {
        return Err("Derived BTC key does not match wallet BTC address".to_string());
    }
    if account.derive_address(change_address.origin)? != *change_address {
        return Err("Bitcoin change address does not belong to this wallet".to_string());
    }
    for utxo in utxos {
        if account.derive_address(utxo.owner.origin)? != utxo.owner {
            return Err("Bitcoin UTXO address does not belong to this wallet".to_string());
        }
    }

    let (selected, fee_sats, change_sats) =
        bitcoin_select_coins(utxos, amount_sats, fee_rate_sat_vb)?;
    let wallet_total = utxos.iter().try_fold(0u64, |total, utxo| {
        total
            .checked_add(utxo.value)
            .ok_or_else(|| "Bitcoin wallet balance overflowed".to_string())
    })?;

    let mut contexts = Vec::<BitcoinSigningContext>::new();
    let mut context_by_origin = BTreeMap::<BitcoinKeyOrigin, usize>::new();
    let mut inputs = Vec::with_capacity(selected.len());
    for utxo in selected {
        let origin = utxo.owner.origin;
        let context_index = if let Some(index) = context_by_origin.get(&origin).copied() {
            let context = contexts
                .get(index)
                .ok_or_else(|| "Bitcoin signing context is missing".to_string())?;
            if context.address != utxo.owner.address {
                return Err("Bitcoin UTXO address does not match its key origin".to_string());
            }
            index
        } else {
            let context = bitcoin_signing_context(mnemonic, &utxo.owner)?;
            let index = contexts.len();
            contexts.push(context);
            context_by_origin.insert(origin, index);
            index
        };
        inputs.push(BitcoinTxInput {
            script_code: contexts[context_index].script_code.clone(),
            signing_context_index: context_index,
            utxo,
        });
    }

    let mut outputs = vec![BitcoinTxOutput {
        value: amount_sats,
        script_pubkey: bitcoin_script_pubkey_from_address(to_address)?,
    }];
    if change_sats > 0 {
        outputs.push(BitcoinTxOutput {
            value: change_sats,
            script_pubkey: bitcoin_script_pubkey_from_address(&change_address.address)?,
        });
    }

    let mut signatures = vec![];
    for (i, input) in inputs.iter().enumerate() {
        let sighash = bitcoin_sighash(i, &inputs, &outputs)?;
        let context = contexts
            .get(input.signing_context_index)
            .ok_or_else(|| "Bitcoin signing context is missing".to_string())?;
        if context.origin != input.utxo.owner.origin || context.address != input.utxo.owner.address
        {
            return Err("Bitcoin input does not match its signing context".to_string());
        }
        let signature: k256::ecdsa::Signature = context
            .signing_key
            .sign_prehash(&sighash)
            .map_err(|_| "Bitcoin transaction signing failed".to_string())?;
        let mut der = signature.to_der().as_bytes().to_vec();
        der.push(0x01);
        signatures.push(der);
    }

    let stripped = bitcoin_serialize_stripped(&inputs, &outputs)?;
    let txid = bitcoin_txid_from_stripped(&stripped);

    let mut raw = vec![];
    raw.extend_from_slice(&2i32.to_le_bytes());
    raw.extend_from_slice(&[0x00, 0x01]);
    raw.extend(bitcoin_varint(inputs.len() as u64));
    for input in &inputs {
        raw.extend(bitcoin_txid_le(&input.utxo.txid)?);
        raw.extend_from_slice(&input.utxo.vout.to_le_bytes());
        raw.push(0x00);
        raw.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
    }
    raw.extend(bitcoin_serialize_outputs(&outputs));
    for (input, sig) in inputs.iter().zip(&signatures) {
        let context = contexts
            .get(input.signing_context_index)
            .ok_or_else(|| "Bitcoin signing context is missing".to_string())?;
        raw.push(0x02);
        raw.extend(bitcoin_push_data(sig));
        raw.extend(bitcoin_push_data(&context.public_key));
    }
    raw.extend_from_slice(&0u32.to_le_bytes());

    Ok(BitcoinSignedTransfer {
        txid,
        raw_tx_hex: hex::encode(raw),
        first_signature_hex: signatures.first().map(hex::encode).unwrap_or_default(),
        fee_sats,
        post_balance: wallet_total
            .checked_sub(amount_sats)
            .and_then(|balance| balance.checked_sub(fee_sats))
            .ok_or_else(|| "Bitcoin post-transaction balance underflowed".to_string())?,
    })
}

fn bitcoin_signing_context(
    mnemonic: &str,
    derived: &BitcoinDerivedAddress,
) -> Result<BitcoinSigningContext, String> {
    let path = derived.origin.derivation_path();
    let mut private_key = secp256k1_private_key_from_mnemonic(mnemonic, &path)?;
    let result = (|| {
        let signing_key = signing_key_from_private_key(&private_key)?;
        let public_key = signing_key
            .verifying_key()
            .to_sec1_point(true)
            .as_bytes()
            .to_vec();
        let pubkey_hash =
            <Ripemd160 as RipemdDigest>::digest(<Sha256 as Sha2Digest>::digest(&public_key));
        let address = segwit::encode_v0(hrp::BC, &pubkey_hash)
            .map_err(|_| "Failed to encode Bitcoin signing address".to_string())?;
        if address != derived.address {
            return Err("Bitcoin key origin does not match its address".to_string());
        }
        Ok(BitcoinSigningContext {
            origin: derived.origin,
            address,
            signing_key,
            public_key,
            script_code: bitcoin_p2pkh_script_code(&pubkey_hash),
        })
    })();
    private_key.zeroize();
    result
}
