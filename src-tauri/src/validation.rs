use crate::address::bitcoin::validate_address as validate_bitcoin_address;
use crate::address::evm::validate_address as validate_evm_address;
use crate::address::filecoin::validate_address as validate_filecoin_address;
use crate::address::injective::validate_address as validate_injective_address;
use crate::address::solana::validate_address as validate_solana_address;
use crate::address::tron::validate_address as validate_tron_address;
use crate::address::zcash::validate_address as validate_zcash_address;
use crate::assets::token_addresses_match;
use crate::dto::Wallet;
use crate::registry::network_by_id;

pub(crate) fn validate_passphrase(passphrase: &str) -> Result<(), String> {
    if passphrase.chars().count() < 8 {
        return Err("Passphrase must be at least 8 characters".to_string());
    }
    Ok(())
}

pub(crate) fn clean_name(name: String) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "Primary Wallet".to_string()
    } else {
        trimmed.chars().take(48).collect()
    }
}

pub(crate) fn validate_transfer(
    wallet: &Wallet,
    to: &str,
    symbol: &str,
    network: &str,
    token_address: Option<&str>,
    amount: &str,
) -> Result<(), String> {
    let network_config =
        network_by_id(network).ok_or_else(|| format!("Unsupported network {network}"))?;
    let to = to.trim();
    if to.is_empty() || to.chars().any(char::is_whitespace) {
        return Err("Recipient address must not be empty or contain whitespace".to_string());
    }
    if amount.is_empty() || !amount.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("Amount must be an unsigned integer in base units".to_string());
    }

    let asset = wallet
        .assets
        .iter()
        .find(|asset| {
            asset.symbol == symbol
                && asset.network == network
                && match (token_address, asset.token_address.as_deref()) {
                    (None, None) => true,
                    (Some(expected), Some(actual)) => {
                        token_addresses_match(network, actual, expected)
                    }
                    _ => false,
                }
        })
        .ok_or_else(|| {
            "Asset not found for the requested network and token identifier".to_string()
        })?;

    match (token_address, asset.token_address.as_deref()) {
        (None, None) if asset.symbol == network_config.native_asset.symbol => {}
        (None, None) => return Err("Native asset symbol does not match its network".to_string()),
        (Some(_), Some(token_address)) => validate_token_identifier(network, token_address)?,
        _ => return Err("Native and token asset identities do not match".to_string()),
    }

    let amount: u128 = amount
        .parse()
        .map_err(|_| "Amount is too large".to_string())?;
    if amount == 0 {
        return Err("Amount must be greater than zero".to_string());
    }
    if matches!(network, "bitcoin" | "solana" | "tron") && amount > u64::MAX as u128 {
        return Err(format!("{symbol} amount is too large"));
    }

    if network != "solana" {
        let balance: u128 = asset
            .balance
            .parse()
            .map_err(|_| "Invalid stored balance".to_string())?;
        if balance < amount {
            return Err(format!("Insufficient {symbol} balance"));
        }
    }
    validate_address_for_network(to, network)?;

    Ok(())
}

fn validate_token_identifier(network: &str, token_address: &str) -> Result<(), String> {
    if token_address.trim() != token_address || token_address.is_empty() {
        return Err(
            "Token identifier must not be empty or contain surrounding whitespace".to_string(),
        );
    }

    match network {
        "solana" => crate::address::solana::validate_pubkey(token_address, "token mint"),
        network if network_by_id(network).is_some_and(|config| config.kind == "evm") => {
            validate_evm_address(token_address)
        }
        "tron" => Err("TRC-20 token transfers are not implemented".to_string()),
        _ => Err(format!("Token transfers are not implemented on {network}")),
    }
}

pub(crate) fn validate_address_for_network(address: &str, network: &str) -> Result<(), String> {
    let config = network_by_id(network).ok_or_else(|| format!("Unsupported network {network}"))?;
    match network {
        "bitcoin" => validate_bitcoin_address(address),
        "filecoin" => validate_filecoin_address(address),
        "injective" => validate_injective_address(address),
        "solana" => validate_solana_address(address),
        "tron" => validate_tron_address(address),
        "zcash" => validate_zcash_address(address),
        _ if config.kind == "evm" => validate_evm_address(address),
        _ => Err(format!("Unsupported network {network}")),
    }
}
