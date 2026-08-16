use bech32::Bech32;
use bech32::primitives::decode::CheckedHrpstring;

pub(crate) fn validate_address(address: &str) -> Result<(), String> {
    let checked = CheckedHrpstring::new::<Bech32>(address)
        .map_err(|_| "Recipient must be a valid Injective bech32 address".to_string())?;
    if checked.hrp().as_str() != "inj" || checked.byte_iter().count() != 20 {
        return Err("Recipient must be an Injective mainnet account address".to_string());
    }
    Ok(())
}
