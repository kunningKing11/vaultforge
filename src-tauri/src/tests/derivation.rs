use bip39::{Language, Mnemonic};

use super::{
    ALL_NETWORKS, BIP39_WORD_COUNTS, derive_addresses_from_mnemonic_filtered,
    validate_recovery_phrase_word_count,
};

#[test]
fn validates_standard_bip39_recovery_phrase_lengths_and_checksums() {
    for (entropy_length, expected_word_count) in [(16, 12), (20, 15), (24, 18), (28, 21), (32, 24)]
    {
        let mnemonic = Mnemonic::from_entropy_in(Language::English, &vec![0; entropy_length])
            .unwrap()
            .to_string();
        assert_eq!(mnemonic.split_whitespace().count(), expected_word_count);
        assert!(BIP39_WORD_COUNTS.contains(&expected_word_count));
        assert!(validate_recovery_phrase_word_count(&mnemonic).is_ok());
        assert!(derive_addresses_from_mnemonic_filtered(&mnemonic, ALL_NETWORKS).is_ok());
    }

    assert!(validate_recovery_phrase_word_count("abandon abandon abandon").is_err());
    assert!(derive_addresses_from_mnemonic_filtered(
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon",
        ALL_NETWORKS,
    )
    .is_err());
}

#[test]
fn derives_documented_wallet_paths_deterministically() {
    let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let addresses = derive_addresses_from_mnemonic_filtered(mnemonic, ALL_NETWORKS).unwrap();
    assert_eq!(addresses.len(), 7);
    assert_eq!(
        addresses.get("bitcoin").unwrap(),
        "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu"
    );
    assert_eq!(
        addresses.get("evm").unwrap(),
        "0x9858effd232b4033e47d90003d41ec34ecaeda94"
    );
    assert_eq!(
        addresses.get("filecoin").unwrap(),
        "f1qode47ievxlxzk6z2viuovedabmn3tq6t57uqhq"
    );
    assert_eq!(
        addresses.get("injective").unwrap(),
        "inj1gsvdpdxec8hsu57lhxg5xem7refr233zkczfgv"
    );
    assert_eq!(
        addresses.get("solana").unwrap(),
        "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk"
    );
    assert_eq!(
        addresses.get("tron").unwrap(),
        "TUEZSdKsoDHQMeZwihtdoBiN46zxhGWYdH"
    );
    assert_eq!(
        addresses.get("zcash").unwrap(),
        "t1XVXWCvpMgBvUaed4XDqWtgQgJSu1Ghz7F"
    );
}
