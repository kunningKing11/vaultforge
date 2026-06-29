use crate::derivation::{BITCOIN_DERIVATION_PATH, secp256k1_private_key_from_mnemonic};
use crate::providers::http::{http_get_json, http_post_text};
use crate::tx::bitcoin::{BitcoinSignedTransfer, bitcoin_signed_transfer};

#[derive(Clone, Debug)]
pub(crate) struct BitcoinUtxo {
    pub(crate) txid: String,
    pub(crate) vout: u32,
    pub(crate) value: u64,
    pub(crate) confirmed: bool,
}

pub(crate) async fn fetch_bitcoin_balance(address: &str) -> Result<String, String> {
    let url = format!("https://blockstream.info/api/address/{address}");
    let json = http_get_json(&url).await?;
    parse_bitcoin_balance(&json).map(|sats| sats.to_string())
}

pub(crate) fn parse_bitcoin_balance(json: &serde_json::Value) -> Result<u128, String> {
    let funded = json["chain_stats"]["funded_txo_sum"].as_u64().unwrap_or(0) as u128;
    let spent = json["chain_stats"]["spent_txo_sum"].as_u64().unwrap_or(0) as u128;
    let mempool_funded = json["mempool_stats"]["funded_txo_sum"]
        .as_u64()
        .unwrap_or(0) as u128;
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
    let items = json
        .as_array()
        .ok_or_else(|| "Bitcoin UTXO response is not an array".to_string())?;
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
        if let Some(rate) = json[target].as_f64()
            && rate.is_finite()
            && rate > 0.0
        {
            return Ok(rate.ceil().max(1.0) as u64);
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

pub(crate) async fn sign_bitcoin_transfer(
    mnemonic: &str,
    from: &str,
    to: &str,
    amount_sats: u64,
) -> Result<BitcoinSignedTransfer, String> {
    let private_key = secp256k1_private_key_from_mnemonic(mnemonic, BITCOIN_DERIVATION_PATH)?;
    let utxos = fetch_bitcoin_utxos(from).await?;
    let fee_rate = fetch_bitcoin_fee_rate().await?;
    bitcoin_signed_transfer(&private_key, from, to, amount_sats, &utxos, fee_rate)
}
