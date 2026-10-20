use super::build_approval_call;

const ROUTER: &str = "0x2222222222222222222222222222222222222222";
const TOKEN: &str = "0x1111111111111111111111111111111111111111";

#[test]
fn encodes_exact_approval_call() {
    let call = build_approval_call(TOKEN, ROUTER, 1_000_000).unwrap();

    assert_eq!(call.to, TOKEN);
    assert_eq!(call.value, 0);
    // Independent ERC-20 ABI fixture: approve's selector, a left-padded
    // spender address, and 1,000,000 (0x0f4240) as a uint256 word.
    assert_eq!(
        hex::encode(&call.data),
        concat!(
            "095ea7b3",
            "0000000000000000000000002222222222222222222222222222222222222222",
            "00000000000000000000000000000000000000000000000000000000000f4240",
        )
    );
}

#[test]
fn encodes_maximum_supported_amount_without_unlimited_approval() {
    let call = build_approval_call(TOKEN, ROUTER, u128::MAX).unwrap();
    assert_eq!(call.data.len(), 68);
    assert_eq!(&call.data[36..52], &[0; 16]);
    assert_eq!(&call.data[52..], &[0xff; 16]);
}

#[test]
fn encodes_single_pool_swap_and_deadline_against_independent_words() {
    let output = "0x3333333333333333333333333333333333333333";
    let recipient = "0x4444444444444444444444444444444444444444";
    let call = super::build_exact_input_single(
        TOKEN, output, ROUTER, recipient, 500, 1_000_000, 1_990_000,
    )
    .unwrap();
    // IV3SwapRouter.ExactInputSingleParams, encoded independently of the builder.
    let expected = format!(
        "04e45aaf{:0>64}{:0>64}{:064x}{:0>64}{:064x}{:064x}{:064x}",
        &TOKEN[2..],
        &output[2..],
        500,
        &recipient[2..],
        1_000_000,
        1_990_000,
        0
    );
    assert_eq!(hex::encode(&call.data), expected);
    let wrapped = super::wrap_swap_with_deadline(call, 1_800_000_600).unwrap();
    let expected = format!(
        "5ae401dc{:064x}{:064x}{:064x}{:064x}{:064x}{expected}{}",
        1_800_000_600,
        64,
        1,
        32,
        228,
        "00".repeat(28)
    );
    assert_eq!(hex::encode(&wrapped.data), expected);
    assert_eq!(wrapped.to, ROUTER);
    let decoded = super::decode_deadline_swap(&wrapped.data).unwrap();
    assert_eq!(decoded.deadline, 1_800_000_600);
    assert_eq!(decoded.amount, 1_000_000);
    assert_eq!(decoded.minimum_received, 1_990_000);
}

#[test]
fn rejects_invalid_and_zero_addresses() {
    for address in [
        "invalid",
        "0x1234",
        "0x0000000000000000000000000000000000000000",
    ] {
        assert!(build_approval_call(address, ROUTER, 1).is_err());
        assert!(build_approval_call(TOKEN, address, 1).is_err());
    }
}

#[test]
fn rejects_swap_fee_overflow_and_invalid_deadline_wrappers() {
    assert!(
        super::build_exact_input_single(TOKEN, ROUTER, ROUTER, TOKEN, 0x100_0000, 1, 0).is_err()
    );
    let call = super::build_exact_input_single(TOKEN, ROUTER, ROUTER, TOKEN, 500, 1, 0).unwrap();
    assert!(super::wrap_swap_with_deadline(call, 0).is_err());
}

#[test]
fn rejects_zero_amount() {
    assert!(build_approval_call(TOKEN, ROUTER, 0).is_err());
}
