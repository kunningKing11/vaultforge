import { invoke } from "@tauri-apps/api/core";
import type { SignedTransaction, WalletSession } from "./types";

export const walletApi = {
  getWallet: () => invoke<WalletSession>("get_wallet"),
  refreshPrices: () => invoke<WalletSession>("refresh_prices"),
  createWallet: (args: { name: string; passphrase: string }) => invoke<WalletSession>("create_wallet", args),
  importWallet: (args: { mnemonic: string; passphrase: string }) => invoke<WalletSession>("import_wallet", args),
  unlockWallet: (args: { passphrase: string }) => invoke<WalletSession>("unlock_wallet", args),
  lockWallet: () => invoke<null>("lock_wallet"),
  clearWallet: () => invoke<WalletSession>("clear_wallet"),
  signTransaction: (args: { to: string; symbol: string; network: string; amount: string; note: string }) => invoke<SignedTransaction>("sign_transaction", args),
  sendTransaction: (args: { signed: SignedTransaction }) => invoke<WalletSession>("send_transaction", args),
  swapTokens: (args: { fromSymbol: string; toSymbol: string; amount: string }) => invoke<WalletSession>("swap_tokens", args),
  checkTransactionStatus: (args: { txHash: string; network: string }) => invoke<string | null>("check_transaction_status", args),
};
