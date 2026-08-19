import { invoke } from "@tauri-apps/api/core";
import type { NetworkId, SignedTransaction, WalletSession } from "./types";

export const walletApi = {
  getWallet: () => invoke<WalletSession>("get_wallet"),
  refreshPrices: () => invoke<WalletSession>("refresh_prices"),
  generateMnemonic: (wordCount?: number) =>
    invoke<string>("generate_mnemonic_cmd", { wordCount: wordCount ?? null }),
  createWallet: (args: {
    name: string;
    walletPassword: string;
    enabledNetworks: string[];
    autoLockTimeoutSecs: number | null;
    mnemonic?: string;
  }) => invoke<WalletSession>("create_wallet", args),
  importWallet: (args: {
    name?: string;
    mnemonic: string;
    walletPassword: string;
    enabledNetworks: string[];
    autoLockTimeoutSecs: number | null;
  }) => invoke<WalletSession>("import_wallet", args),
  unlockWallet: (args: { walletPassword: string }) => invoke<WalletSession>("unlock_wallet", args),
  lockWallet: () => invoke<null>("lock_wallet"),
  clearWallet: () => invoke<WalletSession>("clear_wallet"),
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
