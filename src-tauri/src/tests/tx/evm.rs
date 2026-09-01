use super::{Eip1559TxDraft, encode_erc20_transfer, sign_eip1559_transfer};

#[test]
fn constructs_valid_eip1559_signature() {
    use k256::ecdsa::SigningKey;
    let private_key = [0xabu8; 32];
    let signing_key = SigningKey::from_bytes((&private_key).into()).unwrap();
    let result = sign_eip1559_transfer(&Eip1559TxDraft {
        signing_key: &signing_key,
        chain_id: 1,
        nonce: 0,
        max_priority_fee_per_gas: 1_000_000_000,
        max_fee_per_gas: 1_000_000_000,
        gas_limit: 21000,
        to: "0xdAC17F958D2ee523a2206206994597C13D831ec7",
        value: 1_000_000_000_000_000_000u128,
        data: &[],
    });
    assert!(result.is_ok());
    let (_raw, tx_hash, _raw_hex, r, s) = result.unwrap();
    assert!(tx_hash.starts_with("0x"));
    assert_eq!(tx_hash.len(), 66);
    assert_eq!(r.len(), 64);
    assert_eq!(s.len(), 64);
    assert!(!_raw.is_empty());
}

#[test]
fn encodes_erc20_transfer_abi() {
    let recipient = "0xdAC17F958D2ee523a2206206994597C13D831ec7";
    let amount = 1_000_000_000_000_000_000;
    let data = encode_erc20_transfer(recipient, amount).unwrap();
    assert!(!data.is_empty());
    assert_eq!(data.len(), 4 + 32 + 32);
    assert_eq!(&data[..4], &[0xa9, 0x05, 0x9c, 0xbb]);
    let recip_bytes = hex::decode(recipient.trim_start_matches("0x")).unwrap();
    assert_eq!(&data[16..36], &recip_bytes[..]);
    assert_eq!(data[data.len() - 1], 0x00);
}

#[test]
fn signs_erc20_transfer() {
    use k256::ecdsa::SigningKey;
    let private_key = [0xabu8; 32];
    let signing_key = SigningKey::from_bytes((&private_key).into()).unwrap();
    let data =
        encode_erc20_transfer("0xdAC17F958D2ee523a2206206994597C13D831ec7", 1_000_000).unwrap();
    let result = sign_eip1559_transfer(&Eip1559TxDraft {
        signing_key: &signing_key,
        chain_id: 1,
        nonce: 0,
        max_priority_fee_per_gas: 1_000_000_000,
        max_fee_per_gas: 1_000_000_000,
        gas_limit: 50000,
        to: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
        value: 0,
        data: &data,
    });
    assert!(result.is_ok());
    let (_raw, tx_hash, _raw_hex, r, s) = result.unwrap();
    assert!(tx_hash.starts_with("0x"));
    assert_eq!(tx_hash.len(), 66);
    assert_eq!(r.len(), 64);
    assert_eq!(s.len(), 64);
}
