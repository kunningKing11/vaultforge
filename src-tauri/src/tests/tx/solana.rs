use base64::Engine;

use super::{
    SolanaTokenSource, SolanaTokenTransferDraft, select_solana_token_sources,
    sign_solana_token_transfer_with_blockhash, sign_solana_transfer_with_blockhash,
    solana_associated_token_address,
};
use crate::derivation::{ALL_NETWORKS, derive_addresses_from_mnemonic_filtered};
use crate::providers::solana::SolanaTokenAccount;

#[test]
fn signs_solana_native_transfer() {
    let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let addresses = derive_addresses_from_mnemonic_filtered(mnemonic, ALL_NETWORKS).unwrap();
    let from = addresses.get("solana").unwrap();
    let signed = sign_solana_transfer_with_blockhash(
        mnemonic,
        from,
        "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ",
        1_000_000,
        "11111111111111111111111111111111",
        5000,
    )
    .unwrap();

    assert!(!signed.signature.is_empty());
    assert_eq!(
        signed.raw_tx_base64,
        "Ad6quY5jtfdWLlVxx8ao2zvT6EknQABbIysXyEsl0Gtpb0v0YpwaCUp7UlOcwjbEDCsoTuOIYx2WUqYM1RxyXwUBAAED8DYnYkanW53jNJ7UKxXiMvZRj8IPX81PHWToH5vSWPdgZHxcfemexzdm9yU3fQRHL/i9mRHwgYGLefMeankFrwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAgIAAQwCAAAAQEIPAAAAAAA="
    );
    assert_eq!(signed.recent_blockhash, "11111111111111111111111111111111");
    assert_eq!(signed.fee_lamports, 5000);
}

#[test]
fn signs_solana_spl_token_transfer() {
    let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let addresses = derive_addresses_from_mnemonic_filtered(mnemonic, ALL_NETWORKS).unwrap();
    let from = addresses.get("solana").unwrap();
    let source = SolanaTokenSource {
        address: solana_associated_token_address(
            from,
            "So11111111111111111111111111111111111111112",
        )
        .unwrap(),
        amount: 1_000_000,
    };
    let signed = sign_solana_token_transfer_with_blockhash(SolanaTokenTransferDraft {
        mnemonic,
        from,
        to: "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ",
        mint: "So11111111111111111111111111111111111111112",
        sources: &[source],
        decimals: 9,
        recent_blockhash: "11111111111111111111111111111111",
        fee_lamports: 5000,
    })
    .unwrap();

    assert!(!signed.signature.is_empty());
    assert_eq!(
        signed.raw_tx_base64,
        "ASdwuShdy+hvKd+3RP6ckHTP6BAjEGJLuPwevJRja3Zk3Hb4Q7nwjJ/FHaoVAY4f1E8oRVAwDBqdelbMm0ZBagoBAAUI8DYnYkanW53jNJ7UKxXiMvZRj8IPX81PHWToH5vSWPen/90BBnpik/KR8QwNjg/a6SNBG1AeiRuWANppcU+NcKxDUtSdBJ4UBoX1qj6IDJwis9EKDvzorJnUS4uWuFwSAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAGm4hX/quBhPtof2NGGMA12sQ53BrrO1WYoPAAAAAAAQbd9uHXZaGT2cvhRs7reawctIXtX1s3kTqM9YV+/wCpYGR8XH3pnsc3ZvclN30ERy/4vZkR8IGBi3nzHmp5Ba+MlyWPTiSJ8bs9ECkUjg2DC1oTmdr/EIQEjnvY2+n4WQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgcGAAIGBAMFAQEFBAEEAgAKDEBCDwAAAAAACQ=="
    );
    assert_eq!(signed.fee_lamports, 5000);
}

#[test]
fn combines_classic_spl_token_accounts_with_ata_priority() {
    let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let addresses = derive_addresses_from_mnemonic_filtered(mnemonic, ALL_NETWORKS).unwrap();
    let owner = addresses.get("solana").unwrap();
    let mint = "So11111111111111111111111111111111111111112";
    let ata = solana_associated_token_address(owner, mint).unwrap();
    let accounts = vec![
        SolanaTokenAccount {
            address: "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ".to_string(),
            mint: mint.to_string(),
            amount: 7,
            decimals: 9,
        },
        SolanaTokenAccount {
            address: ata.clone(),
            mint: mint.to_string(),
            amount: 3,
            decimals: 9,
        },
    ];

    let (sources, total) = select_solana_token_sources(owner, mint, 9, 8, &accounts).unwrap();
    assert_eq!(total, 10);
    assert_eq!(
        sources,
        vec![
            SolanaTokenSource {
                address: ata,
                amount: 3,
            },
            SolanaTokenSource {
                address: "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ".to_string(),
                amount: 5,
            },
        ]
    );
    assert!(select_solana_token_sources(owner, mint, 9, 11, &accounts).is_err());

    let signed = sign_solana_token_transfer_with_blockhash(SolanaTokenTransferDraft {
        mnemonic,
        from: owner,
        to: "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ",
        mint,
        sources: &sources,
        decimals: 9,
        recent_blockhash: "11111111111111111111111111111111",
        fee_lamports: 5000,
    })
    .unwrap();
    let raw = base64::engine::general_purpose::STANDARD
        .decode(signed.raw_tx_base64)
        .unwrap();
    let transaction: solana_transaction::Transaction = wincode::deserialize(&raw).unwrap();
    assert_eq!(transaction.message.instructions.len(), 3);
}

#[test]
fn derives_solana_associated_token_address() {
    let ata = solana_associated_token_address(
        "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ",
        "So11111111111111111111111111111111111111112",
    )
    .unwrap();

    assert!(!ata.is_empty());
}
