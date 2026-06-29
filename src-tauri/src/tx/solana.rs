pub(crate) async fn sign_solana_transfer(
    _mnemonic: &str,
    _from: &str,
    _to: &str,
    _lamports: u64,
) -> Result<String, String> {
    Err("Signing Solana transactions is not supported yet".to_string())
}
