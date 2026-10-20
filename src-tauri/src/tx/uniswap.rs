use sha3::{Digest, Keccak256};

use crate::tx::evm::{EvmCall, evm_address_word, evm_uint_word};

pub(crate) struct DecodedUniswapSwap {
    pub(crate) token_in: [u8; 32],
    pub(crate) token_out: [u8; 32],
    pub(crate) recipient: [u8; 32],
    pub(crate) amount: u128,
    pub(crate) minimum_received: u128,
    pub(crate) deadline: u64,
}

/// Reads only a canonical deadline multicall containing one SwapRouter02 swap.
/// Exact lengths and offsets reject extra calls, nested wrappers, and trailing data.
pub(crate) fn decode_deadline_swap(data: &[u8]) -> Result<DecodedUniswapSwap, String> {
    // 4-byte selector, five ABI words, and one 228-byte call padded to 256 bytes.
    if data.len() != 420 || data[..4] != Keccak256::digest(b"multicall(uint256,bytes[])")[..4] {
        return Err("Swap must be one deadline-wrapped SwapRouter02 call".into());
    }
    let word = |offset: usize| -> [u8; 32] {
        data[offset..offset + 32]
            .try_into()
            .expect("checked ABI length")
    };
    if word(36) != evm_uint_word(64)
        || word(68) != evm_uint_word(1)
        || word(100) != evm_uint_word(32)
        || word(132) != evm_uint_word(228)
        || data[392..].iter().any(|byte| *byte != 0)
    {
        return Err("Swap wrapper must contain exactly one canonically encoded call".into());
    }
    if data[164..168]
        != Keccak256::digest(
            b"exactInputSingle((address,address,uint24,address,uint256,uint256,uint160))",
        )[..4]
    {
        return Err("Unsupported swap entrypoint".into());
    }
    if abi_u128(word(232))? > 0xff_ffff || word(360) != [0; 32] {
        return Err("Invalid pool fee or unsupported swap price limit".into());
    }
    let deadline =
        u64::try_from(abi_u128(word(4))?).map_err(|_| "Swap deadline exceeds supported range")?;
    Ok(DecodedUniswapSwap {
        token_in: word(168),
        token_out: word(200),
        recipient: word(264),
        amount: abi_u128(word(296))?,
        minimum_received: abi_u128(word(328))?,
        deadline,
    })
}

fn abi_u128(word: [u8; 32]) -> Result<u128, String> {
    if word[..16].iter().any(|byte| *byte != 0) {
        return Err("Swap integer exceeds supported range".into());
    }
    Ok(u128::from_be_bytes(
        word[16..].try_into().expect("16-byte integer"),
    ))
}

/// Builds an approval for exactly `amount` base units of `token_in`.
/// The caller supplies the router from `uniswap_config`.
pub(crate) fn build_approval_call(
    token_in: &str,
    router: &str,
    amount: u128,
) -> Result<EvmCall, String> {
    let token_word = evm_address_word(token_in)?;
    let router_word = evm_address_word(router)?;
    if token_word == [0; 32] || router_word == [0; 32] {
        return Err("Approval token and router must be nonzero addresses".into());
    }
    if amount == 0 {
        return Err("Approval amount must be greater than zero".into());
    }

    // approve(address,uint256): selector, spender address, exact token amount.
    let mut data = Keccak256::digest(b"approve(address,uint256)")[..4].to_vec();
    data.extend_from_slice(&router_word);
    data.extend_from_slice(&evm_uint_word(amount));

    Ok(EvmCall {
        to: token_in.into(),
        value: 0,
        data,
    })
}

/// Builds a SwapRouter02 call to swap exactly `amount` base units of `token_in`
/// for `token_out` through one V3 pool, sending at least `minimum_received`
/// base units of the output token to `recipient` if execution succeeds.
/// The caller supplies the router from `uniswap_config` and adds the expiry wrapper.
/// This only builds the call; it does not sign or send it.
pub(crate) fn build_exact_input_single(
    token_in: &str,
    token_out: &str,
    router: &str,
    recipient: &str,
    pool_fee: u32,
    amount: u128,
    minimum_received: u128,
) -> Result<EvmCall, String> {
    let token_in_word = evm_address_word(token_in)?;
    let token_out_word = evm_address_word(token_out)?;
    let router_word = evm_address_word(router)?;
    let recipient_word = evm_address_word(recipient)?;

    if token_in_word == [0; 32]
        || token_out_word == [0; 32]
        || router_word == [0; 32]
        || recipient_word == [0; 32]
    {
        return Err(
            "Swap input token, output token, router, and recipient addresses must be nonzero"
                .into(),
        );
    }
    if amount == 0 {
        return Err("Swap input amount must be greater than zero".into());
    }
    if token_in_word == token_out_word {
        return Err("Swap input and output tokens must be different".into());
    }

    if pool_fee > 0xff_ffff {
        return Err("Swap pool fee must fit in 24 bits".into());
    }

    // SwapRouter02 exactInputSingle selector, followed by its seven tuple fields.
    let mut data = Keccak256::digest(
        b"exactInputSingle((address,address,uint24,address,uint256,uint256,uint160))",
    )[..4]
        .to_vec();

    data.extend_from_slice(&token_in_word);
    data.extend_from_slice(&token_out_word);
    data.extend_from_slice(&evm_uint_word(pool_fee as u128));
    data.extend_from_slice(&recipient_word);
    data.extend_from_slice(&evm_uint_word(amount));
    data.extend_from_slice(&evm_uint_word(minimum_received));
    data.extend_from_slice(&evm_uint_word(0)); // No explicit sqrtPriceLimitX96 limit.

    Ok(EvmCall {
        to: router.into(),
        value: 0,
        data,
    })
}

#[cfg(test)]
#[path = "../tests/tx/uniswap.rs"]
mod tests;

/// Gives a single swap an on-chain Unix expiry without changing its inner bytes.
pub(crate) fn wrap_swap_with_deadline(call: EvmCall, deadline: u64) -> Result<EvmCall, String> {
    if call.value != 0 || call.data.len() != 228 || deadline == 0 {
        return Err("Expected a single ERC-20 swap and a nonzero expiry".into());
    }
    let mut data = Keccak256::digest(b"multicall(uint256,bytes[])")[..4].to_vec();
    for word in [deadline as u128, 64, 1, 32, 228] {
        data.extend_from_slice(&evm_uint_word(word));
    }
    data.extend_from_slice(&call.data);
    data.resize(420, 0);
    Ok(EvmCall {
        to: call.to,
        value: 0,
        data,
    })
}

/// Encodes a read-only QuoterV2 single-pool price request, not a swap transaction.
pub(crate) fn build_pool_quote_call(
    token_in: &str,
    token_out: &str,
    quoter: &str,
    amount: u128,
    pool_fee: u32,
) -> Result<EvmCall, String> {
    let mut data =
        Keccak256::digest(b"quoteExactInputSingle((address,address,uint256,uint24,uint160))")[..4]
            .to_vec();
    data.extend_from_slice(&evm_address_word(token_in)?);
    data.extend_from_slice(&evm_address_word(token_out)?);
    data.extend_from_slice(&evm_uint_word(amount));
    data.extend_from_slice(&evm_uint_word(pool_fee.into()));
    data.extend_from_slice(&evm_uint_word(0));
    evm_address_word(quoter)?;
    Ok(EvmCall {
        to: quoter.into(),
        value: 0,
        data,
    })
}
