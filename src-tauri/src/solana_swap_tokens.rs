//! Verified token-program and fee terms retained with a Solana swap quote.
use crate::dto::SolanaTransferFeeSchedule;
use crate::solana_tokens::{TokenDescriptor, TokenProgram, inspect_holding, inspect_mint};
use crate::swap_solana::SolanaRpc;
use crate::tx::swap_solana::{NATIVE, WSOL, key, mint};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct SwapToken {
    pub(crate) mint: String,
    pub(crate) program: String,
    pub(crate) decimals: u8,
    pub(crate) extensions: Vec<u16>,
    pub(crate) fee_schedule: Option<SolanaTransferFeeSchedule>,
    pub(crate) account_len: usize,
}
impl SwapToken {
    pub(crate) fn from_descriptor(
        descriptor: &TokenDescriptor,
        epoch: u64,
    ) -> Result<Self, String> {
        if let Some(reason) = descriptor.unsupported_reason() {
            return Err(reason);
        }
        let mut extensions = descriptor
            .extensions
            .iter()
            .copied()
            .map(u16::from)
            .collect::<Vec<_>>();
        extensions.sort_unstable();
        let fee_schedule = descriptor.transfer_fee.as_ref().map(|config| {
            let fee = config.get_epoch_fee(epoch);
            SolanaTransferFeeSchedule {
                epoch: u64::from(fee.epoch).to_string(),
                basis_points: u16::from(fee.transfer_fee_basis_points),
                maximum_fee: u64::from(fee.maximum_fee).to_string(),
            }
        });
        Ok(Self {
            mint: descriptor.mint.clone(),
            program: descriptor.program.id().into(),
            decimals: descriptor.decimals,
            extensions,
            fee_schedule,
            account_len: descriptor.account_len()?,
        })
    }
    pub(crate) fn associated(&self, owner: &str) -> Result<solana_pubkey::Pubkey, String> {
        Ok(spl_associated_token_account_interface::address::get_associated_token_address_with_program_id(
            &key(owner)?, &key(&self.mint)?, &key(&self.program)?))
    }
    pub(crate) fn fee(&self, amount: u64) -> Result<u64, String> {
        let Some(fee) = &self.fee_schedule else {
            return Ok(0);
        };
        let fee = spl_token_2022_interface::extension::transfer_fee::TransferFee {
            epoch: fee
                .epoch
                .parse::<u64>()
                .map_err(|_| "Invalid swap fee epoch")?
                .into(),
            maximum_fee: fee
                .maximum_fee
                .parse::<u64>()
                .map_err(|_| "Invalid swap fee cap")?
                .into(),
            transfer_fee_basis_points: fee.basis_points.into(),
        };
        fee.calculate_fee(amount)
            .ok_or("Swap transfer fee overflow".into())
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        let program = TokenProgram::parse(&self.program)?;
        key(&self.mint)?;
        if self.mint == WSOL && (program != TokenProgram::Classic || self.decimals != 9) {
            return Err("Wrapped SOL program or decimals changed".into());
        }
        if self.account_len < 165
            || (program == TokenProgram::Classic
                && (self.account_len != 165
                    || !self.extensions.is_empty()
                    || self.fee_schedule.is_some()))
        {
            return Err("Invalid saved Solana token terms".into());
        }
        if self
            .fee_schedule
            .as_ref()
            .is_some_and(|f| f.basis_points > 10000)
        {
            return Err("Invalid swap transfer fee".into());
        }
        self.fee(1)?;
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct SwapTokens {
    pub(crate) input: SwapToken,
    pub(crate) output: SwapToken,
}
impl SwapTokens {
    pub(crate) fn validate(&self, input: &str, output: &str) -> Result<(), String> {
        self.input.validate()?;
        self.output.validate()?;
        if self.input.mint != mint(input) || self.output.mint != mint(output) {
            return Err("Saved swap token identities changed".into());
        }
        Ok(())
    }
    pub(crate) fn get(&self, token: &str) -> Result<&SwapToken, String> {
        if self.input.mint == mint(token) {
            Ok(&self.input)
        } else if self.output.mint == mint(token) {
            Ok(&self.output)
        } else {
            Err("Unreviewed swap token mint".into())
        }
    }
}
pub(crate) async fn load(
    client: &(impl SolanaRpc + Sync),
    input: &str,
    output: &str,
) -> Result<SwapTokens, String> {
    let response = client
        .swap_rpc(
            "getMultipleAccounts",
            json!([[mint(input),mint(output)],{"encoding":"base64","commitment":"confirmed"}]),
        )
        .await?;
    let values = response["value"]
        .as_array()
        .filter(|v| v.len() == 2)
        .ok_or("Swap mint accounts missing")?;
    let descriptors = [
        inspect_mint(mint(input), &values[0])?,
        inspect_mint(mint(output), &values[1])?,
    ];
    let epoch = if descriptors.iter().any(|d| d.transfer_fee.is_some()) {
        client
            .swap_rpc("getEpochInfo", json!([{"commitment":"confirmed"}]))
            .await?["epoch"]
            .as_u64()
            .ok_or("Swap fee epoch missing")?
    } else {
        0
    };
    let tokens = SwapTokens {
        input: SwapToken::from_descriptor(&descriptors[0], epoch)?,
        output: SwapToken::from_descriptor(&descriptors[1], epoch)?,
    };
    tokens.validate(input, output)?;
    Ok(tokens)
}
pub(crate) async fn recheck(
    client: &(impl SolanaRpc + Sync),
    tokens: &SwapTokens,
    sender: &str,
    input: &str,
    output: &str,
    amount: u128,
) -> Result<(), String> {
    tokens.validate(input, output)?;
    if &load(client, input, output).await? != tokens {
        return Err("Solana token program, extensions or active transfer fee changed; request a fresh quote".into());
    }
    for (token, native, is_input) in [
        (&tokens.input, input == NATIVE, true),
        (&tokens.output, output == NATIVE, false),
    ] {
        if native {
            continue;
        }
        let address = token.associated(sender)?.to_string();
        let response = client
            .swap_rpc(
                "getAccountInfo",
                json!([address,{"encoding":"base64","commitment":"confirmed"}]),
            )
            .await?;
        let value = response
            .get("value")
            .ok_or("Swap token account response missing")?;
        if value.is_null() && !is_input {
            continue;
        }
        let holding = inspect_holding(&address, sender, value)?;
        if holding.mint != token.mint || holding.program.id() != token.program {
            return Err("Swap token account identity changed".into());
        }
        if let Some(reason) = holding.unsupported_reason() {
            return Err(reason);
        }
        if is_input && u128::from(holding.amount) < amount {
            return Err("Insufficient live swap input balance in the wallet ATA".into());
        }
    }
    Ok(())
}
