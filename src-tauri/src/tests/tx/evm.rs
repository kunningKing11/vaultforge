use super::{Eip1559TxDraft, encode_erc20_transfer, sign_eip1559_transfer};
use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use sha3::{Digest, Keccak256};

#[test]
fn constructs_valid_eip1559_signature() {
    let private_key = [0xabu8; 32];
    let signing_key = SigningKey::from_bytes((&private_key).into()).unwrap();
    let draft = Eip1559TxDraft {
        signing_key: &signing_key,
        chain_id: 1,
        nonce: 0,
        max_priority_fee_per_gas: 1_000_000_000,
        max_fee_per_gas: 1_000_000_000,
        gas_limit: 21000,
        to: "0xdAC17F958D2ee523a2206206994597C13D831ec7",
        value: 1_000_000_000_000_000_000u128,
        data: &[],
    };
    let signed = sign_eip1559_transfer(&draft).unwrap();
    assert_signed_transaction(&draft, &signed);
    let (_raw, tx_hash, _raw_hex, r, s) = signed;
    assert!(tx_hash.starts_with("0x"));
    assert_eq!(tx_hash.len(), 66);
    assert_eq!(r.len(), 64);
    assert_eq!(s.len(), 64);
    assert!(!_raw.is_empty());
}

#[test]
fn encodes_eip1559_integer_boundaries() {
    let signing_key = SigningKey::from_bytes((&[0xabu8; 32]).into()).unwrap();
    for value in [0, 1, 127, 128, 255, 256, u128::MAX] {
        let draft = Eip1559TxDraft {
            signing_key: &signing_key,
            chain_id: 1,
            nonce: 0,
            max_priority_fee_per_gas: value,
            max_fee_per_gas: value,
            gas_limit: 21_000,
            to: "0x1111111111111111111111111111111111111111",
            value,
            data: &[],
        };
        let signed = sign_eip1559_transfer(&draft).unwrap();
        assert_signed_transaction(&draft, &signed);
        if value == 0 {
            let decoded = rlp::Rlp::new(&signed.0[1..]);
            for index in [1, 2, 3, 6] {
                assert_eq!(decoded.at(index).unwrap().as_raw(), &[0x80]);
            }
        }
    }
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
fn encodes_signature_scalars_without_leading_zeroes() {
    let signing_key = SigningKey::from_bytes((&[0xabu8; 32]).into()).unwrap();
    for (nonce, scalar_index) in [(88, 1), (444, 0)] {
        let draft = Eip1559TxDraft {
            signing_key: &signing_key,
            chain_id: 1,
            nonce,
            max_priority_fee_per_gas: 1_000_000_000,
            max_fee_per_gas: 1_000_000_000,
            gas_limit: 21_000,
            to: "0x1111111111111111111111111111111111111111",
            value: 1,
            data: &[],
        };
        let signed = sign_eip1559_transfer(&draft).unwrap();
        let scalar = [&signed.3, &signed.4][scalar_index];
        assert!(scalar.starts_with("00"));
        let decoded = rlp::Rlp::new(&signed.0[1..]);
        assert_eq!(
            decoded.at(10 + scalar_index).unwrap().data().unwrap().len(),
            31
        );
        assert_signed_transaction(&draft, &signed);
    }
}

#[test]
fn matches_canonical_zero_value_signing_payload() {
    let signing_key = SigningKey::from_bytes((&[0xabu8; 32]).into()).unwrap();
    let draft = Eip1559TxDraft {
        signing_key: &signing_key,
        chain_id: 1,
        nonce: 0,
        max_priority_fee_per_gas: 1_000_000_000,
        max_fee_per_gas: 1_000_000_000,
        gas_limit: 21_000,
        to: "0x1111111111111111111111111111111111111111",
        value: 0,
        data: &[],
    };
    let signed = sign_eip1559_transfer(&draft).unwrap();
    let decoded = rlp::Rlp::new(&signed.0[1..]);
    assert_eq!(
        hex::encode(signing_payload(&decoded)),
        "02e70180843b9aca00843b9aca008252089411111111111111111111111111111111111111118080c0",
    );
    assert_signed_transaction(&draft, &signed);
}

#[test]
fn signs_erc20_transfer() {
    let private_key = [0xabu8; 32];
    let signing_key = SigningKey::from_bytes((&private_key).into()).unwrap();
    let data =
        encode_erc20_transfer("0xdAC17F958D2ee523a2206206994597C13D831ec7", 1_000_000).unwrap();
    let draft = Eip1559TxDraft {
        signing_key: &signing_key,
        chain_id: 1,
        nonce: 0,
        max_priority_fee_per_gas: 1_000_000_000,
        max_fee_per_gas: 1_000_000_000,
        gas_limit: 50000,
        to: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
        value: 0,
        data: &data,
    };
    let signed = sign_eip1559_transfer(&draft).unwrap();
    assert_signed_transaction(&draft, &signed);
    let (_raw, tx_hash, _raw_hex, r, s) = signed;
    assert!(tx_hash.starts_with("0x"));
    assert_eq!(tx_hash.len(), 66);
    assert_eq!(r.len(), 64);
    assert_eq!(s.len(), 64);
}

fn assert_signed_transaction(
    draft: &Eip1559TxDraft,
    signed: &(Vec<u8>, String, String, String, String),
) {
    let (raw, tx_hash, raw_hex, r_hex, s_hex) = signed;
    assert_eq!(raw[0], 0x02);
    assert_eq!(raw_hex, &format!("0x{}", hex::encode(raw)));
    assert_eq!(
        tx_hash,
        &format!("0x{}", hex::encode(Keccak256::digest(raw)))
    );
    let decoded = rlp::Rlp::new(&raw[1..]);
    assert_eq!(decoded.item_count().unwrap(), 12);
    assert_eq!(decoded.val_at::<u64>(0).unwrap(), draft.chain_id);
    assert_eq!(decoded.val_at::<u64>(1).unwrap(), draft.nonce);
    assert_eq!(
        decoded.val_at::<u128>(2).unwrap(),
        draft.max_priority_fee_per_gas
    );
    assert_eq!(decoded.val_at::<u128>(3).unwrap(), draft.max_fee_per_gas);
    assert_eq!(decoded.val_at::<u64>(4).unwrap(), draft.gas_limit);
    assert_eq!(
        decoded.val_at::<Vec<u8>>(5).unwrap(),
        hex::decode(&draft.to[2..]).unwrap()
    );
    assert_eq!(decoded.val_at::<u128>(6).unwrap(), draft.value);
    assert_eq!(decoded.val_at::<Vec<u8>>(7).unwrap(), draft.data);
    assert!(decoded.at(8).unwrap().is_list());
    assert_eq!(decoded.at(8).unwrap().item_count().unwrap(), 0);
    let parity = decoded.val_at::<u8>(9).unwrap();
    assert!(parity <= 1);

    let mut scalars = [[0u8; 32]; 2];
    for (index, scalar) in scalars.iter_mut().enumerate() {
        let bytes = decoded.at(10 + index).unwrap().data().unwrap();
        assert!(!bytes.is_empty() && bytes.len() <= 32);
        assert_ne!(bytes[0], 0, "signature scalar has a leading zero");
        scalar[32 - bytes.len()..].copy_from_slice(bytes);
    }
    assert_eq!(hex::encode(scalars[0]), *r_hex);
    assert_eq!(hex::encode(scalars[1]), *s_hex);
    let signature = Signature::from_scalars(scalars[0], scalars[1]).unwrap();
    let digest = Keccak256::digest(signing_payload(&decoded));
    let recovered = VerifyingKey::recover_from_prehash(
        &digest,
        &signature,
        RecoveryId::new(parity == 1, false),
    )
    .unwrap();
    assert_eq!(&recovered, draft.signing_key.verifying_key());
}

fn signing_payload(decoded: &rlp::Rlp<'_>) -> Vec<u8> {
    let mut unsigned = rlp::RlpStream::new_list(9);
    for index in 0..9 {
        unsigned.append_raw(decoded.at(index).unwrap().as_raw(), 1);
    }
    let mut payload = vec![0x02];
    payload.extend_from_slice(&unsigned.out());
    payload
}
