//! LI.FI GenericSwapFacetV3 intent verification. Unsupported bridge/facet calls fail closed.
use crate::tx::evm::{EvmCall, evm_address_word};
use sha3::{Digest, Keccak256};

pub(crate) const DIAMOND: &str = "0x1231deb6f5749ef6ce6943a275a1d3e7486f4eae";
fn word(data: &[u8], index: usize) -> Result<&[u8], String> {
    data.get(
        index.checked_mul(32).ok_or("LI.FI offset overflow")?
            ..index
                .checked_add(1)
                .and_then(|n| n.checked_mul(32))
                .ok_or("LI.FI offset overflow")?,
    )
    .ok_or("Truncated LI.FI ABI".into())
}
fn number(data: &[u8], index: usize) -> Result<u128, String> {
    let word = word(data, index)?;
    if word[..16].iter().any(|b| *b != 0) {
        return Err("LI.FI integer exceeds wallet amount range".into());
    }
    Ok(u128::from_be_bytes(
        word[16..].try_into().map_err(|_| "Invalid LI.FI integer")?,
    ))
}
fn address(data: &[u8], index: usize) -> Result<[u8; 32], String> {
    let value: [u8; 32] = word(data, index)?
        .try_into()
        .map_err(|_| "Invalid LI.FI address")?;
    if value[..12] != [0; 12] {
        return Err("Malformed LI.FI address".into());
    }
    Ok(value)
}

/// The pinned facet pulls exactly the selected input and enforces the final
/// output asset, minimum, and recipient atomically. It also checks its on-chain
/// target/selector allowlist. We accept only bounded same-chain single/multiple entrypoints with one input deposit.
pub(crate) fn verify_call(
    call: &EvmCall,
    sender: &str,
    token_in: &str,
    token_out: &str,
    input: u128,
    minimum: u128,
) -> Result<(), String> {
    if !call.to.eq_ignore_ascii_case(DIAMOND) || call.data.len() > 65_536 {
        return Err("LI.FI route uses an unsupported execution contract".into());
    }
    let bytes = &call.data;
    let native_in = evm_address_word(token_in)? == [0; 32];
    let native_out = evm_address_word(token_out)? == [0; 32];
    if native_in && native_out {
        return Err("Choose different swap assets".into());
    }
    let suffix = match (native_in, native_out) {
        (false, false) => "ERC20ToERC20",
        (false, true) => "ERC20ToNative",
        (true, false) => "NativeToERC20",
        _ => unreachable!(),
    };
    let selector = |multiple: bool| {
        let kind = if multiple { "Multiple" } else { "Single" };
        let array = if multiple { "[]" } else { "" };
        Keccak256::digest(format!("swapTokens{kind}V3{suffix}(bytes32,string,string,address,uint256,(address,address,address,address,uint256,bytes,bool){array})").as_bytes())
    };
    let multiple = bytes.get(..4) == Some(&selector(true)[..4]);
    if !multiple && bytes.get(..4) != Some(&selector(false)[..4]) {
        return Err("No supported LI.FI route: unsupported facet or bridge instructions".into());
    }
    let data = bytes.get(4..).ok_or("Truncated LI.FI calldata")?;
    if address(data, 3)? != evm_address_word(sender)? || number(data, 4)? < minimum || minimum == 0
    {
        return Err("LI.FI recipient or minimum output changed".into());
    }
    let offset = usize::try_from(number(data, 5)?).map_err(|_| "LI.FI ABI offset overflow")?;
    if offset < 192 || offset % 32 != 0 {
        return Err("Invalid LI.FI swap offset".into());
    }
    let swaps = data.get(offset..).ok_or("LI.FI swap missing")?;
    let count = if multiple {
        usize::try_from(number(swaps, 0)?).map_err(|_| "Too many LI.FI steps")?
    } else {
        1
    };
    if !(1..=8).contains(&count) {
        return Err("Unsupported LI.FI swap step count".into());
    }
    let mut previous_asset = evm_address_word(token_in)?;
    for i in 0..count {
        let swap = if multiple {
            let relative = usize::try_from(number(swaps, i + 1)?)
                .map_err(|_| "LI.FI tuple offset overflow")?;
            if relative < count * 32 || relative % 32 != 0 {
                return Err("Invalid LI.FI tuple offset".into());
            }
            swaps
                .get(
                    32usize
                        .checked_add(relative)
                        .ok_or("LI.FI offset overflow")?..,
                )
                .ok_or("LI.FI tuple missing")?
        } else {
            swaps
        };
        if address(swap, 2)? != previous_asset
            || (i == 0 && number(swap, 4)? != input)
            || number(swap, 4)? == 0
            || address(swap, 0)? != address(swap, 1)?
            || address(swap, 0)? == [0; 32]
            || number(swap, 6)? > 1
            || (multiple
                && ((i > 0 && number(swap, 6)? != 0)
                    || (i == 0 && !native_in && number(swap, 6)? != 1)))
        {
            return Err("LI.FI input, output, spender, or amount changed".into());
        }
        previous_asset = address(swap, 3)?;
        let nested_offset =
            usize::try_from(number(swap, 5)?).map_err(|_| "LI.FI nested offset overflow")?;
        if nested_offset < 224 || nested_offset % 32 != 0 {
            return Err("Invalid LI.FI nested calldata".into());
        }
        let nested = swap
            .get(nested_offset..)
            .ok_or("LI.FI nested calldata missing")?;
        let length =
            usize::try_from(number(nested, 0)?).map_err(|_| "LI.FI calldata length overflow")?;
        if length < 4
            || nested
                .get(32..32usize.checked_add(length).ok_or("LI.FI length overflow")?)
                .is_none()
        {
            return Err("Truncated LI.FI swap call".into());
        }
    }
    if previous_asset != evm_address_word(token_out)? {
        return Err("LI.FI final output asset changed".into());
    }
    if call.value != if native_in { input } else { 0 } {
        return Err("LI.FI native value differs from selected amount".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/tx/lifi.rs"]
pub(crate) mod tests;
