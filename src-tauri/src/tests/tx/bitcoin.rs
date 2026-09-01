use super::{bitcoin_estimated_vbytes, bitcoin_select_coins, bitcoin_signed_transfer};
use crate::derivation::{
    BitcoinAccount, BitcoinKeyOrigin, secp256k1_private_key_from_mnemonic,
    signing_key_from_private_key,
};
use crate::tests::{BITCOIN_TEST_MNEMONIC, bitcoin_test_owner, bitcoin_test_utxo};

#[test]
fn derives_standard_bip84_receive_and_change_addresses() {
    let account = BitcoinAccount::from_mnemonic(BITCOIN_TEST_MNEMONIC).unwrap();
    assert_eq!(
        account.primary_address().unwrap().address,
        "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu"
    );
    assert_eq!(
        account
            .derive_address(BitcoinKeyOrigin::change(0))
            .unwrap()
            .address,
        "bc1q8c6fshw2dlwun7ekn9qwf37cu2rn755upcp6el"
    );
}

#[test]
fn selects_bitcoin_coins_with_change() {
    let utxos = vec![bitcoin_test_utxo(
        1,
        50_000,
        true,
        BitcoinKeyOrigin::external(0),
    )];
    let (selected, fee, change) = bitcoin_select_coins(&utxos, 10_000, 2).unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(fee, bitcoin_estimated_vbytes(1, 2) * 2);
    assert_eq!(change, 50_000 - 10_000 - fee);
}

#[test]
fn signs_bitcoin_p2wpkh_transfer() {
    let from = bitcoin_test_owner(BitcoinKeyOrigin::external(0));
    let utxos = vec![bitcoin_test_utxo(
        1,
        50_000,
        true,
        BitcoinKeyOrigin::external(0),
    )];
    let signed = bitcoin_signed_transfer(
        BITCOIN_TEST_MNEMONIC,
        &from.address,
        "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq",
        10_000,
        &utxos,
        2,
        &from,
    )
    .unwrap();

    assert_eq!(signed.txid.len(), 64);
    assert!(signed.raw_tx_hex.starts_with("020000000001"));
    assert!(!signed.first_signature_hex.is_empty());
    assert_eq!(signed.fee_sats, bitcoin_estimated_vbytes(1, 2) * 2);
    assert_eq!(signed.post_balance, 50_000 - 10_000 - signed.fee_sats);
}

#[test]
fn signs_bitcoin_inputs_from_different_bip84_paths() {
    let from = bitcoin_test_owner(BitcoinKeyOrigin::external(0));
    let second_owner = bitcoin_test_owner(BitcoinKeyOrigin::change(1));
    let utxos = vec![
        bitcoin_test_utxo(1, 6_000, true, BitcoinKeyOrigin::external(0)),
        bitcoin_test_utxo(2, 5_000, true, BitcoinKeyOrigin::change(1)),
    ];
    let signed = bitcoin_signed_transfer(
        BITCOIN_TEST_MNEMONIC,
        &from.address,
        "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq",
        10_000,
        &utxos,
        2,
        &from,
    )
    .unwrap();

    let raw = signed.raw_tx_hex;
    for owner in [&from, &second_owner] {
        let private_key = secp256k1_private_key_from_mnemonic(
            BITCOIN_TEST_MNEMONIC,
            &owner.origin.derivation_path(),
        )
        .unwrap();
        let public_key = signing_key_from_private_key(&private_key)
            .unwrap()
            .verifying_key()
            .to_sec1_point(true);
        assert!(raw.contains(&hex::encode(public_key.as_bytes())));
    }
    assert_eq!(signed.post_balance, 11_000 - 10_000 - signed.fee_sats);
}

#[test]
fn rejects_bitcoin_utxo_with_forged_key_origin() {
    let from = bitcoin_test_owner(BitcoinKeyOrigin::external(0));
    let mut forged = bitcoin_test_utxo(1, 50_000, true, BitcoinKeyOrigin::external(0));
    forged.owner.address = bitcoin_test_owner(BitcoinKeyOrigin::change(0)).address;
    let error = bitcoin_signed_transfer(
        BITCOIN_TEST_MNEMONIC,
        &from.address,
        "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq",
        10_000,
        &[forged],
        2,
        &from,
    )
    .err()
    .unwrap();
    assert!(error.contains("does not belong"));
}

#[test]
fn bitcoin_coin_selection_handles_no_change_and_segwit_dust() {
    let amount = 10_000;
    let fee_rate = 2;
    let fee_no_change = bitcoin_estimated_vbytes(1, 1) * fee_rate;
    let fee_with_change = bitcoin_estimated_vbytes(1, 2) * fee_rate;

    let no_change = vec![bitcoin_test_utxo(
        1,
        amount + fee_no_change + 10,
        true,
        BitcoinKeyOrigin::external(0),
    )];
    let (_, fee, change) = bitcoin_select_coins(&no_change, amount, fee_rate).unwrap();
    assert_eq!(fee, fee_no_change + 10);
    assert_eq!(change, 0);

    let exact_dust = vec![bitcoin_test_utxo(
        2,
        amount + fee_with_change + 294,
        true,
        BitcoinKeyOrigin::external(0),
    )];
    let (_, fee, change) = bitcoin_select_coins(&exact_dust, amount, fee_rate).unwrap();
    assert_eq!(fee, fee_with_change);
    assert_eq!(change, 294);
}
