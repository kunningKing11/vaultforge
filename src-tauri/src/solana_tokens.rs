//! Program-aware token inspection. Execution paths must still opt in explicitly.

use base64::{Engine, engine::general_purpose::STANDARD};
use solana_pubkey::Pubkey;
use spl_token_2022_interface::{
    extension::{
        AccountType, BaseStateWithExtensions, ExtensionType, StateWithExtensions,
        transfer_fee::TransferFeeConfig,
    },
    state::{Account, AccountState, Mint},
};
use std::str::FromStr;

pub(crate) const CLASSIC: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub(crate) const TOKEN_2022: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenProgram {
    Classic,
    Token2022,
}
impl TokenProgram {
    pub(crate) fn parse(owner: &str) -> Result<Self, String> {
        match owner {
            CLASSIC => Ok(Self::Classic),
            TOKEN_2022 => Ok(Self::Token2022),
            _ => Err("Unsupported Solana token owner program".into()),
        }
    }
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Classic => CLASSIC,
            Self::Token2022 => TOKEN_2022,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TokenDescriptor {
    pub(crate) mint: String,
    pub(crate) program: TokenProgram,
    pub(crate) decimals: u8,
    pub(crate) extensions: Vec<ExtensionType>,
    pub(crate) required_account_extensions: Vec<ExtensionType>,
    pub(crate) transfer_fee: Option<TransferFeeConfig>,
}
impl TokenDescriptor {
    pub(crate) fn associated_address(&self, owner: &str) -> Result<String, String> {
        let parse =
            |s: &str| Pubkey::from_str(s).map_err(|_| "Invalid Solana token address".to_string());
        Ok(spl_associated_token_account_interface::address::get_associated_token_address_with_program_id(
            &parse(owner)?, &parse(&self.mint)?, &parse(self.program.id())?).to_string())
    }
    pub(crate) fn account_len(&self) -> Result<usize, String> {
        if self.program == TokenProgram::Classic {
            return Ok(165);
        }
        let mut extensions = self.required_account_extensions.clone();
        // The ATA program initializes immutable ownership for Token-2022 accounts.
        extensions.push(ExtensionType::ImmutableOwner);
        ExtensionType::try_calculate_account_len::<Account>(&extensions)
            .map_err(|e| format!("Cannot size Solana token account: {e}"))
    }
    pub(crate) fn unsupported_reason(&self) -> Option<String> {
        self.extensions
            .iter()
            .find(|e| {
                !matches!(
                    e,
                    ExtensionType::MetadataPointer
                        | ExtensionType::TokenMetadata
                        | ExtensionType::GroupPointer
                        | ExtensionType::GroupMemberPointer
                        | ExtensionType::TokenGroup
                        | ExtensionType::TokenGroupMember
                        | ExtensionType::MintCloseAuthority
                        | ExtensionType::TransferFeeConfig
                )
            })
            .map(|e| format!("Unsupported Solana mint extension: {e:?}"))
    }
    pub(crate) fn fee(&self, epoch: u64, amount: u64) -> Result<u64, String> {
        self.transfer_fee
            .as_ref()
            .map_or(Some(0), |config| config.calculate_epoch_fee(epoch, amount))
            .ok_or_else(|| "Solana token transfer fee overflow".into())
    }
}

pub(crate) fn account_data(value: &serde_json::Value) -> Result<(TokenProgram, Vec<u8>), String> {
    let program = TokenProgram::parse(
        value["owner"]
            .as_str()
            .ok_or("Missing Solana account program")?,
    )?;
    if value["executable"].as_bool() != Some(false) || value["data"][1] != "base64" {
        return Err("Invalid Solana token account encoding or executable state".into());
    }
    let bytes = STANDARD
        .decode(
            value["data"][0]
                .as_str()
                .ok_or("Missing Solana account data")?,
        )
        .map_err(|_| "Invalid Solana account base64".to_string())?;
    Ok((program, bytes))
}

pub(crate) fn inspect_mint(
    mint: &str,
    value: &serde_json::Value,
) -> Result<TokenDescriptor, String> {
    Pubkey::from_str(mint).map_err(|_| "Invalid Solana mint address")?;
    let (program, bytes) = account_data(value)?;
    if program == TokenProgram::Classic && bytes.len() != 82 {
        return Err("Invalid classic mint size".into());
    }
    let state = StateWithExtensions::<Mint>::unpack(&bytes)
        .map_err(|e| format!("Invalid Solana mint: {e}"))?;
    let extensions = state
        .get_extension_types()
        .map_err(|e| format!("Invalid mint extensions: {e}"))?;
    validate_extensions(&bytes, &extensions, AccountType::Mint)?;
    ExtensionType::check_for_invalid_mint_extension_combinations(&extensions)
        .map_err(|e| format!("Invalid mint extension combination: {e}"))?;
    if extensions.contains(&ExtensionType::TokenMetadata) {
        state
            .get_variable_len_extension::<spl_token_metadata_interface::state::TokenMetadata>()
            .map_err(|e| format!("Invalid token metadata extension: {e}"))?;
    }
    let transfer_fee = if extensions.contains(&ExtensionType::TransferFeeConfig) {
        let config = *state
            .get_extension::<TransferFeeConfig>()
            .map_err(|e| format!("Invalid transfer fee: {e}"))?;
        if u16::from(config.older_transfer_fee.transfer_fee_basis_points) > 10_000
            || u16::from(config.newer_transfer_fee.transfer_fee_basis_points) > 10_000
        {
            return Err("Invalid Solana transfer fee basis points".into());
        }
        Some(config)
    } else {
        None
    };
    let mut required_account_extensions = Vec::new();
    spl_token_2022_interface::extension::account_len::try_for_each_required_init_account_extension(
        state.get_tlv_data(),
        |extension| {
            required_account_extensions.push(extension);
            Ok(())
        },
    )
    .map_err(|e| format!("Invalid required token extensions: {e}"))?;
    Ok(TokenDescriptor {
        mint: mint.into(),
        program,
        decimals: state.base.decimals,
        extensions,
        required_account_extensions,
        transfer_fee,
    })
}

// Validate lengths and duplicates as well as TLV tags. The generic extension walker
// alone does not decode each extension payload. Metadata is decoded separately.
fn validate_extensions(
    bytes: &[u8],
    extensions: &[ExtensionType],
    kind: AccountType,
) -> Result<(), String> {
    let mut seen = Vec::new();
    let mut offset = 166;
    for extension in extensions {
        if extension.get_account_type() != kind || seen.contains(extension) {
            return Err("Invalid or duplicate Solana token extension".into());
        }
        seen.push(*extension);
        let length = u16::from_le_bytes(
            bytes
                .get(offset + 2..offset + 4)
                .ok_or("Truncated token extension")?
                .try_into()
                .map_err(|_| "Invalid token extension length")?,
        ) as usize;
        let end = offset
            .checked_add(4 + length)
            .ok_or("Token extension overflow")?;
        bytes
            .get(offset..end)
            .ok_or("Truncated token extension payload")?;
        if !matches!(extension, ExtensionType::TokenMetadata) {
            let expected = ExtensionType::try_calculate_account_len::<Account>(&[*extension])
                .map_err(|e| format!("Invalid token extension size: {e}"))?;
            // A single fixed extension occupies the shared 166-byte prefix plus tag/length.
            let actual = 166 + 4 + length;
            if expected != actual && !(actual == 355 && expected == 357) {
                return Err("Invalid Solana token extension payload size".into());
            }
        }
        offset = end;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TokenHolding {
    pub(crate) address: String,
    pub(crate) mint: String,
    pub(crate) program: TokenProgram,
    pub(crate) amount: u64,
    pub(crate) frozen: bool,
    pub(crate) extensions: Vec<ExtensionType>,
}
impl TokenHolding {
    pub(crate) fn unsupported_reason(&self) -> Option<String> {
        if self.frozen {
            return Some("Solana token account is frozen".into());
        }
        self.extensions
            .iter()
            .find(|e| {
                !matches!(
                    e,
                    ExtensionType::ImmutableOwner | ExtensionType::TransferFeeAmount
                )
            })
            .map(|e| format!("Unsupported Solana account extension: {e:?}"))
    }
}
pub(crate) fn inspect_holding(
    address: &str,
    owner: &str,
    value: &serde_json::Value,
) -> Result<TokenHolding, String> {
    Pubkey::from_str(address).map_err(|_| "Invalid Solana token account address")?;
    let (program, bytes) = account_data(value)?;
    if program == TokenProgram::Classic && bytes.len() != 165 {
        return Err("Invalid classic account size".into());
    }
    let state = StateWithExtensions::<Account>::unpack(&bytes)
        .map_err(|e| format!("Invalid Solana token account: {e}"))?;
    if state.base.owner.to_string() != owner {
        return Err("Solana token account owner differs from wallet".into());
    }
    let extensions = state
        .get_extension_types()
        .map_err(|e| format!("Invalid account extensions: {e}"))?;
    validate_extensions(&bytes, &extensions, AccountType::Account)?;
    Ok(TokenHolding {
        address: address.into(),
        mint: state.base.mint.to_string(),
        program,
        amount: state.base.amount,
        frozen: state.base.state == AccountState::Frozen,
        extensions,
    })
}

#[cfg(test)]
#[path = "tests/solana_tokens.rs"]
pub(crate) mod tests;
