use bech32::hrp;
use bech32::segwit;

pub(crate) fn validate_address(address: &str) -> Result<(), String> {
    if address.starts_with("bc1") {
        let (decoded_hrp, version, program) = segwit::decode(address)
            .map_err(|_| "Recipient must be a valid Bitcoin bech32 address".to_string())?;
        if decoded_hrp == hrp::BC && version == segwit::VERSION_0 && program.len() == 20 {
            return Ok(());
        }
        return Err("Only mainnet Bitcoin P2WPKH recipients are supported".to_string());
    }

    let decoded = bs58::decode(address)
        .with_check(None)
        .into_vec()
        .map_err(|_| "Recipient must be a valid Bitcoin base58 address".to_string())?;
    if decoded.len() != 21 {
        return Err("Unsupported Bitcoin base58 recipient length".to_string());
    }
    match decoded[0] {
        0x00 | 0x05 => Ok(()),
        _ => Err("Only mainnet Bitcoin recipients are supported".to_string()),
    }
}
