use super::{required_xrpl_reserve_drops, sign_xrpl_payment};
use crate::derivation::xrpl_classic_address_from_private_key;
use crate::providers::xrpl::{XrplAccountInfo, XrplNetworkInfo};

fn network() -> XrplNetworkInfo {
    XrplNetworkInfo {
        base_reserve_drops: 1_000_000,
        owner_reserve_drops: 200_000,
        open_ledger_fee_drops: 12,
        validated_ledger_index: 95_000_000,
    }
}

#[test]
fn reserves_account_ownership_and_never_spends_it() {
    let account = XrplAccountInfo {
        balance_drops: 1_400_012,
        owner_count: 2,
        sequence: 7,
    };
    assert_eq!(
        required_xrpl_reserve_drops(&account, &network()).unwrap(),
        1_400_000
    );
    let private_key = [1; 32];
    let address = xrpl_classic_address_from_private_key(&private_key).unwrap();
    assert!(
        sign_xrpl_payment(
            &private_key,
            &address,
            "rG1QQv2nh2gr7RCZ1P8YYcBUKCCN633jCn",
            1,
            None,
            "",
            &account,
            &network(),
        )
        .is_err()
    );
}

#[test]
fn constructs_canonical_signed_payment_with_tag_and_memo() {
    let private_key = [1; 32];
    let from = xrpl_classic_address_from_private_key(&private_key).unwrap();
    let payment = sign_xrpl_payment(
        &private_key,
        &from,
        "rG1QQv2nh2gr7RCZ1P8YYcBUKCCN633jCn",
        1_000_000,
        Some(42),
        "invoice-42",
        &XrplAccountInfo {
            balance_drops: 20_000_000,
            owner_count: 0,
            sequence: 7,
        },
        &network(),
    )
    .unwrap();

    assert_eq!(payment.fee_drops, 12);
    assert_eq!(payment.sequence, 7);
    assert_eq!(payment.reserve_drops, 1_000_000);
    assert_eq!(payment.post_balance_drops, 18_999_988);
    assert!(payment.raw_tx_hex.starts_with("12"));
    assert!(payment.signature.starts_with("30"));
    assert_eq!(payment.tx_hash.len(), 64);
    let decoded = xrpl::core::binarycodec::decode(&payment.raw_tx_hex).unwrap();
    assert_eq!(decoded["TransactionType"], "Payment");
    assert_eq!(decoded["DestinationTag"], 42);
    assert_eq!(
        decoded["Memos"][0]["Memo"]["MemoData"],
        "696E766F6963652D3432"
    );
}
