use crate::derivation::*;
use crate::providers::http::{http_get_json, http_post_text};
use bech32::{self, FromBase32, Variant};
use k256::ecdsa::signature::hazmat::PrehashSigner;
use ripemd::Ripemd160;
use sha2::{Digest as Sha2Digest, Sha256};

#[derive(Clone, Debug)]
pub(crate) struct BitcoinUtxo {
    pub(crate) txid: String,
    pub(crate) vout: u32,
    pub(crate) value: u64,
    pub(crate) confirmed: bool,
}

#[derive(Clone)]
pub(crate) struct BitcoinTxInput {
    pub(crate) utxo: BitcoinUtxo,
    pub(crate) script_code: Vec<u8>,
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

pub(crate) async fn fetch_bitcoin_balance(address: &str) -> Result<String, String> {
    let url = format!("https://blockstream.info/api/address/{address}");
    let json = http_get_json(&url).await?;
    parse_bitcoin_balance(&json).map(|sats| sats.to_string())
}

pub(crate) fn parse_bitcoin_balance(json: &serde_json::Value) -> Result<u128, String> {
    let funded = json["chain_stats"]["funded_txo_sum"].as_u64().unwrap_or(0) as u128;
    let spent = json["chain_stats"]["spent_txo_sum"].as_u64().unwrap_or(0) as u128;
    let mempool_funded = json["mempool_stats"]["funded_txo_sum"].as_u64().unwrap_or(0) as u128;
    let mempool_spent = json["mempool_stats"]["spent_txo_sum"].as_u64().unwrap_or(0) as u128;

    let confirmed = funded.saturating_sub(spent);
    let mempool = mempool_funded.saturating_sub(mempool_spent);
    Ok(confirmed + mempool)
}

pub(crate) async fn fetch_bitcoin_utxos(address: &str) -> Result<Vec<BitcoinUtxo>, String> {
    let url = format!("https://blockstream.info/api/address/{address}/utxo");
    let json = http_get_json(&url).await?;
    parse_bitcoin_utxos(&json)
}

pub(crate) fn parse_bitcoin_utxos(json: &serde_json::Value) -> Result<Vec<BitcoinUtxo>, String> {
    let items = json.as_array().ok_or_else(|| "Bitcoin UTXO response is not an array".to_string())?;
    let mut utxos = vec![];
    for item in items {
        let txid = item["txid"]
            .as_str()
            .ok_or_else(|| "Bitcoin UTXO missing txid".to_string())?
            .to_string();
        if txid.len() != 64 || !txid.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("Bitcoin UTXO txid is invalid".to_string());
        }
        let vout = item["vout"]
            .as_u64()
            .ok_or_else(|| "Bitcoin UTXO missing vout".to_string())?;
        let value = item["value"]
            .as_u64()
            .ok_or_else(|| "Bitcoin UTXO missing value".to_string())?;
        if value < 546 {
            continue;
        }
        let confirmed = item["status"]["confirmed"].as_bool().unwrap_or(false);
        utxos.push(BitcoinUtxo {
            txid,
            vout: u32::try_from(vout).map_err(|_| "Bitcoin UTXO vout is too large".to_string())?,
            value,
            confirmed,
        });
    }
    utxos.sort_by(|a, b| b.confirmed.cmp(&a.confirmed).then(a.value.cmp(&b.value)));
    Ok(utxos)
}

pub(crate) async fn fetch_bitcoin_fee_rate() -> Result<u64, String> {
    let json = http_get_json("https://blockstream.info/api/fee-estimates").await?;
    parse_bitcoin_fee_rate(&json)
}

pub(crate) fn parse_bitcoin_fee_rate(json: &serde_json::Value) -> Result<u64, String> {
    for target in ["3", "6", "12", "1"] {
        if let Some(rate) = json[target].as_f64() {
            if rate.is_finite() && rate > 0.0 {
                return Ok(rate.ceil().max(1.0) as u64);
            }
        }
    }
    Err("Bitcoin fee estimate response missing usable fee rate".to_string())
}

pub(crate) async fn broadcast_bitcoin_transaction(raw_tx_hex: &str) -> Result<String, String> {
    http_post_text("https://blockstream.info/api/tx", raw_tx_hex)
        .await
        .map(|txid| txid.trim().to_string())
}

pub(crate) async fn fetch_bitcoin_tx_status(txid: &str) -> Result<Option<String>, String> {
    let url = format!("https://blockstream.info/api/tx/{txid}/status");
    let json = http_get_json(&url).await?;
    if json["confirmed"].as_bool().unwrap_or(false) {
        Ok(Some("confirmed".to_string()))
    } else {
        Ok(None)
    }
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
        let (hrp, data, variant) = bech32::decode(address)
            .map_err(|_| "Invalid Bitcoin bech32 recipient".to_string())?;
        if hrp != "bc" || variant != Variant::Bech32 || data.is_empty() {
            return Err("Unsupported Bitcoin bech32 recipient".to_string());
        }
        let version = data[0].to_u8();
        let program = Vec::<u8>::from_base32(&data[1..])
            .map_err(|_| "Invalid Bitcoin witness program".to_string())?;
        if version != 0 || program.len() != 20 {
            return Err("Only mainnet P2WPKH bc1 recipients are supported".to_string());
        }
        return Ok(bitcoin_p2wpkh_script_pubkey(&program));
    }

    let decoded = bs58::decode(address).with_check(None).into_vec()
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
    let first = Sha256::digest(data);
    Sha256::digest(first).into()
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

fn bitcoin_serialize_stripped(inputs: &[BitcoinTxInput], outputs: &[BitcoinTxOutput]) -> Result<Vec<u8>, String> {
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

fn bitcoin_sighash(input_index: usize, inputs: &[BitcoinTxInput], outputs: &[BitcoinTxOutput]) -> Result<[u8; 32], String> {
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
    let input = inputs.get(input_index).ok_or_else(|| "Bitcoin input index out of range".to_string())?;

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

pub(crate) fn bitcoin_estimated_vbytes(input_count: usize, output_count: usize) -> u64 {
    10 + (input_count as u64 * 68) + (output_count as u64 * 34)
}

pub(crate) fn bitcoin_select_coins(utxos: &[BitcoinUtxo], amount: u64, fee_rate_sat_vb: u64) -> Result<(Vec<BitcoinUtxo>, u64, u64), String> {
    let mut selected = vec![];
    let mut total = 0u64;
    for utxo in utxos.iter().filter(|u| u.confirmed).chain(utxos.iter().filter(|u| !u.confirmed)) {
        selected.push(utxo.clone());
        total = total.saturating_add(utxo.value);
        let fee_with_change = bitcoin_estimated_vbytes(selected.len(), 2).saturating_mul(fee_rate_sat_vb);
        if total >= amount.saturating_add(fee_with_change) {
            let change = total - amount - fee_with_change;
            if change < 546 {
                let fee_no_change = bitcoin_estimated_vbytes(selected.len(), 1).saturating_mul(fee_rate_sat_vb);
                if total >= amount.saturating_add(fee_no_change) {
                    return Ok((selected, total - amount, 0));
                }
            }
            return Ok((selected, fee_with_change, change));
        }
    }
    Err("Insufficient BTC balance for amount plus fee".to_string())
}

pub(crate) fn bitcoin_signed_transfer(
    private_key: &[u8; 32],
    from_address: &str,
    to_address: &str,
    amount_sats: u64,
    utxos: &[BitcoinUtxo],
    fee_rate_sat_vb: u64,
) -> Result<BitcoinSignedTransfer, String> {
    if amount_sats == 0 {
        return Err("Amount must be greater than zero".to_string());
    }

    let signing_key = signing_key_from_private_key(private_key)?;
    let public_key = signing_key.verifying_key().to_encoded_point(true);
    let public_key_bytes = public_key.as_bytes();
    let pubkey_hash = Ripemd160::digest(&Sha256::digest(public_key_bytes));
    let expected_from = bitcoin_bech32_address(private_key, false)?;
    if from_address != expected_from {
        return Err("Derived BTC key does not match wallet BTC address".to_string());
    }

    let (selected, fee_sats, change_sats) = bitcoin_select_coins(utxos, amount_sats, fee_rate_sat_vb)?;
    let total_in: u64 = selected.iter().map(|u| u.value).sum();
    let mut outputs = vec![BitcoinTxOutput {
        value: amount_sats,
        script_pubkey: bitcoin_script_pubkey_from_address(to_address)?,
    }];
    if change_sats > 0 {
        outputs.push(BitcoinTxOutput {
            value: change_sats,
            script_pubkey: bitcoin_p2wpkh_script_pubkey(&pubkey_hash),
        });
    }

    let script_code = bitcoin_p2pkh_script_code(&pubkey_hash);
    let inputs: Vec<BitcoinTxInput> = selected
        .into_iter()
        .map(|utxo| BitcoinTxInput { utxo, script_code: script_code.clone() })
        .collect();

    let mut signatures = vec![];
    for i in 0..inputs.len() {
        let sighash = bitcoin_sighash(i, &inputs, &outputs)?;
        let signature: k256::ecdsa::Signature = signing_key
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
    for sig in &signatures {
        raw.push(0x02);
        raw.extend(bitcoin_push_data(sig));
        raw.extend(bitcoin_push_data(public_key_bytes));
    }
    raw.extend_from_slice(&0u32.to_le_bytes());

    Ok(BitcoinSignedTransfer {
        txid,
        raw_tx_hex: hex::encode(raw),
        first_signature_hex: signatures.first().map(hex::encode).unwrap_or_default(),
        fee_sats,
        post_balance: total_in.saturating_sub(amount_sats).saturating_sub(fee_sats),
    })
}

pub(crate) async fn sign_bitcoin_transfer(mnemonic: &str, from: &str, to: &str, amount_sats: u64) -> Result<BitcoinSignedTransfer, String> {
    let private_key = secp256k1_private_key_from_mnemonic(mnemonic, BITCOIN_DERIVATION_PATH)?;
    let utxos = fetch_bitcoin_utxos(from).await?;
    let fee_rate = fetch_bitcoin_fee_rate().await?;
    bitcoin_signed_transfer(&private_key, from, to, amount_sats, &utxos, fee_rate)
}
