import { invoke } from "@tauri-apps/api/core";

import type {
  AssetIdentity,
  CctpAttestationProgress,
  CctpPreparedStep,
  CctpState,
  FiatCurrency,
  NetworkId,
  SignedTransaction,
  TxStatus,
  WalletRefreshResult,
  WalletSession,
} from "./types";

export const walletApi = {
  getWallet: () => invoke<WalletSession>("get_wallet"),
  refreshPortfolio: () => invoke<WalletRefreshResult>("refresh_portfolio"),
  generateMnemonic: (wordCount?: number) =>
    invoke<string>("generate_mnemonic_cmd", { wordCount: wordCount ?? null }),
  createWallet: (args: {
    name: string;
    walletPassword: string;
    fiatCurrency: FiatCurrency;
    enabledNetworks: string[];
    autoLockTimeoutSecs: number | null;
    useCryptoSymbols: boolean;
    mnemonic?: string;
    bip39Passphrase: string;
  }) => invoke<WalletRefreshResult>("create_wallet", args),
  importWallet: (args: {
    name?: string;
    mnemonic: string;
    bip39Passphrase: string;
    walletPassword: string;
    fiatCurrency: FiatCurrency;
    enabledNetworks: string[];
    autoLockTimeoutSecs: number | null;
    useCryptoSymbols: boolean;
  }) => invoke<WalletRefreshResult>("import_wallet", args),
  unlockWallet: (args: { walletPassword: string }) =>
    invoke<WalletRefreshResult>("unlock_wallet", args),
  lockWallet: () => invoke<null>("lock_wallet"),
  clearWallet: () => invoke<WalletSession>("clear_wallet"),
  updateWalletSettings: (args: {
    name: string;
    fiatCurrency: FiatCurrency;
    autoLockTimeoutSecs: number | null;
    useCryptoSymbols: boolean;
    hideBelowUsd: number;
  }) => invoke<WalletSession>("update_wallet_settings", args),
  signTransaction: (args: {
    to: string;
    symbol: string;
    network: NetworkId;
    tokenAddress: string | null;
    amount: string;
    note: string;
    destinationTag: number | null;
  }) => invoke<SignedTransaction>("sign_transaction", args),
  cancelSignedTransaction: (args: { network: NetworkId; from: string; nonce: string }) =>
    invoke<null>("cancel_signed_transaction", args),
  sendTransaction: (args: { signed: SignedTransaction }) =>
    invoke<WalletSession>("send_transaction", args),
  swapTokens: (args: { fromAsset: AssetIdentity; toAsset: AssetIdentity; amount: string }) =>
    invoke<WalletSession>("swap_tokens", args),
  getCctpState: () => invoke<CctpState>("get_cctp_state"),
  createCctpTransfer: (args: {
    sourceNetwork: string;
    destinationNetwork: string;
    recipient: string;
    amountBaseUnits: string;
  }) => invoke<{ operation_id: string; session: WalletSession }>("create_cctp_transfer", args),
  prepareCctpStep: (args: { operationId: string }) =>
    invoke<CctpPreparedStep>("prepare_cctp_step", args),
  submitCctpStep: (args: { operationId: string; expectedHash: string }) =>
    invoke<WalletSession>("submit_cctp_step", args),
  cancelCctpTransfer: (args: { operationId: string }) =>
    invoke<WalletSession>("cancel_cctp_transfer", args),
  syncCctpTransfer: (args: { operationId: string }) =>
    invoke<WalletSession>("sync_cctp_transfer", args),
  refreshCctpAttestation: (args: { operationId: string }) =>
    invoke<CctpAttestationProgress>("refresh_cctp_attestation", args),
  pollCctpAttestation: (args: { operationId: string }) =>
    invoke<CctpAttestationProgress>("poll_cctp_attestation", args),
  checkTransactionStatus: (args: { txHash: string; network: NetworkId }) =>
    invoke<TxStatus | null>("check_transaction_status", args),
};
