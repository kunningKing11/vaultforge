use sha3::Digest;

use super::{build_approval_call, build_burn_call, build_mint_call};
use crate::cctp::{CctpEnvironment, tests::transfer};
use crate::tx::evm::{Eip1559TxDraft, sign_eip1559_transfer};

#[test]
fn encodes_complete_calls_against_independent_abi_fixtures() {
    // Offline Python ABI encoder and Keccak implementation, independent of Rust.
    // depositForBurn parameters follow Circle's V2 contract interface.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/cctp/calldata.json")).unwrap();
    let transfer = transfer(CctpEnvironment::Mainnet);
    let message: Vec<u8> = (1..=33).collect();
    let attestation: Vec<u8> = (1..=65).collect();
    for (key, call, target) in [
        (
            "approve",
            build_approval_call(&transfer).unwrap(),
            transfer.route.source.usdc,
        ),
        (
            "burn",
            build_burn_call(&transfer).unwrap(),
            transfer.route.source.token_messenger,
        ),
        (
            "mint",
            build_mint_call(&transfer, &message, &attestation).unwrap(),
            transfer.route.destination.message_transmitter,
        ),
    ] {
        assert_eq!(hex::encode(call.data), fixture[key].as_str().unwrap());
        assert_eq!(call.to, target);
        assert_eq!(call.value, 0);
    }
}

#[test]
fn rejects_empty_and_oversized_mint_payloads() {
    let transfer = transfer(CctpEnvironment::Mainnet);
    for (message, attestation) in [
        (vec![], vec![1]),
        (vec![1], vec![]),
        (vec![1; 16385], vec![1]),
        (vec![1], vec![1; 16385]),
    ] {
        assert!(build_mint_call(&transfer, &message, &attestation).is_err());
    }
}

#[test]
fn signs_cctp_calls_with_shared_eip1559_encoding() {
    let transfer = transfer(CctpEnvironment::Sepolia);
    let key = k256::ecdsa::SigningKey::from_slice(&[1; 32]).unwrap();
    for (call, chain) in [
        (build_approval_call(&transfer).unwrap(), 11155111),
        (build_burn_call(&transfer).unwrap(), 11155111),
        (
            build_mint_call(&transfer, &[1; 32], &[2; 65]).unwrap(),
            421614,
        ),
    ] {
        let (raw, _, _, _, _) = sign_eip1559_transfer(&Eip1559TxDraft {
            signing_key: &key,
            chain_id: chain,
            nonce: 1,
            max_priority_fee_per_gas: 1,
            max_fee_per_gas: 2,
            gas_limit: 300000,
            to: &call.to,
            value: call.value,
            data: &call.data,
        })
        .unwrap();
        let tx = rlp::Rlp::new(&raw[1..]);
        assert_eq!(tx.val_at::<u64>(0).unwrap(), chain);
        assert_eq!(
            tx.val_at::<Vec<u8>>(5).unwrap(),
            hex::decode(&call.to[2..]).unwrap()
        );
        assert!(tx.at(6).unwrap().data().unwrap().is_empty());
        assert_eq!(tx.val_at::<Vec<u8>>(7).unwrap(), call.data);
        // Recover the signer from the decoded transaction, not builder metadata.
        let mut unsigned = rlp::RlpStream::new_list(9);
        for index in 0..9 {
            unsigned.append_raw(tx.at(index).unwrap().as_raw(), 1);
        }
        let mut payload = vec![0x02];
        payload.extend_from_slice(&unsigned.out());
        let mut scalars = [[0; 32]; 2];
        for (index, scalar) in scalars.iter_mut().enumerate() {
            let bytes = tx.at(10 + index).unwrap().data().unwrap();
            scalar[32 - bytes.len()..].copy_from_slice(bytes);
        }
        let signature = k256::ecdsa::Signature::from_scalars(scalars[0], scalars[1]).unwrap();
        let parity = tx.val_at::<u8>(9).unwrap();
        let digest = sha3::Keccak256::digest(&payload);
        let recovered = k256::ecdsa::VerifyingKey::recover_from_prehash(
            &digest,
            &signature,
            k256::ecdsa::RecoveryId::new(parity == 1, false),
        )
        .unwrap();
        assert_eq!(&recovered, key.verifying_key());
    }
}
