use k256::ecdsa::signature::hazmat::PrehashSigner;
use sha3::{Digest, Keccak256};

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

pub(crate) fn encode_erc20_transfer(recipient: &str, amount: u128) -> Result<Vec<u8>, String> {
    let recip_hex = recipient.trim_start_matches("0x");
    let recip_bytes =
        hex::decode(recip_hex).map_err(|_| "Invalid recipient address".to_string())?;
    let mut padded_recip = vec![0u8; 32];
    padded_recip[32 - recip_bytes.len()..].copy_from_slice(&recip_bytes);

    let amount_bytes = amount.to_be_bytes();
    let start = amount_bytes
        .iter()
        .position(|&b| b != 0)
        .unwrap_or(amount_bytes.len() - 1);
    let amount_trimmed = &amount_bytes[start..];

    let mut data = vec![0xa9, 0x05, 0x9c, 0xbb]; // keccak256("transfer(address,uint256)")[..4]
    data.extend_from_slice(&padded_recip);
    let mut padded_amount = vec![0u8; 32];
    padded_amount[32 - amount_trimmed.len()..].copy_from_slice(amount_trimmed);
    data.extend_from_slice(&padded_amount);
    Ok(data)
}

fn u128_to_be_bytes(value: u128) -> Vec<u8> {
    let be = value.to_be_bytes();
    let start = be.iter().position(|&b| b != 0).unwrap_or(be.len() - 1);
    be[start..].to_vec()
}

pub(crate) fn sign_eip1559_transfer(
    draft: &Eip1559TxDraft,
) -> Result<(Vec<u8>, String, String, String, String), String> {
    let to_bytes = hex::decode(draft.to.trim_start_matches("0x"))
        .map_err(|_| "Invalid to address".to_string())?;

    let max_priority_bytes = u128_to_be_bytes(draft.max_priority_fee_per_gas);
    let max_fee_bytes = u128_to_be_bytes(draft.max_fee_per_gas);
    let value_bytes = u128_to_be_bytes(draft.value);

    let mut stream = rlp::RlpStream::new();
    stream.begin_list(9);
    stream.append(&draft.chain_id);
    stream.append(&draft.nonce);
    stream.append(&max_priority_bytes);
    stream.append(&max_fee_bytes);
    stream.append(&draft.gas_limit);
    stream.append(&to_bytes);
    stream.append(&value_bytes);
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
    let r_vec: Vec<u8> = r_bytes.to_vec();
    let s_vec: Vec<u8> = s_bytes.to_vec();

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
    tx_stream.append(&max_priority_bytes);
    tx_stream.append(&max_fee_bytes);
    tx_stream.append(&draft.gas_limit);
    tx_stream.append(&to_bytes);
    tx_stream.append(&value_bytes);
    tx_stream.append(&draft.data.to_vec());
    tx_stream.begin_list(0);
    tx_stream.append(&y_parity);
    tx_stream.append(&r_vec);
    tx_stream.append(&s_vec);

    let mut signed_data = vec![0x02u8];
    signed_data.extend_from_slice(&tx_stream.out());

    let tx_hash = format!("0x{}", hex::encode(Keccak256::digest(&signed_data)));
    let raw_tx_hex = format!("0x{}", hex::encode(&signed_data));
    let r_hex = hex::encode(r_bytes);
    let s_hex = hex::encode(s_bytes);

    Ok((signed_data, tx_hash, raw_tx_hex, r_hex, s_hex))
}
