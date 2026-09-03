import { invoke } from "@tauri-apps/api/core";

import type {
  FiatCurrency,
  NetworkId,
  SignedTransaction,
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
    mnemonic?: string;
  }) => invoke<WalletRefreshResult>("create_wallet", args),
  importWallet: (args: {
    name?: string;
    mnemonic: string;
    walletPassword: string;
    fiatCurrency: FiatCurrency;
    enabledNetworks: string[];
    autoLockTimeoutSecs: number | null;
  }) => invoke<WalletRefreshResult>("import_wallet", args),
  unlockWallet: (args: { walletPassword: string }) =>
    invoke<WalletRefreshResult>("unlock_wallet", args),
  lockWallet: () => invoke<null>("lock_wallet"),
  clearWallet: () => invoke<WalletSession>("clear_wallet"),
  setFiatCurrency: (currency: FiatCurrency) =>
    invoke<WalletSession>("set_fiat_currency", { currency }),
  signTransaction: (args: {
    to: string;
    symbol: string;
    network: NetworkId;
    tokenAddress: string | null;
    amount: string;
    note: string;
  }) => invoke<SignedTransaction>("sign_transaction", args),
  sendTransaction: (args: { signed: SignedTransaction }) =>
    invoke<WalletSession>("send_transaction", args),
  swapTokens: (args: { fromSymbol: string; toSymbol: string; amount: string }) =>
    invoke<WalletSession>("swap_tokens", args),
  checkTransactionStatus: (args: { txHash: string; network: NetworkId }) =>
    invoke<string | null>("check_transaction_status", args),
};
