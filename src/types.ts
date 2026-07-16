export type Asset = {
  symbol: string;
  name: string;
  balance: string;
  decimals: number;
  price_usd: number;
  change_24h: number;
  network: NetworkId;
  token_address?: string | null;
};

export type Activity = {
  id: string;
  kind: string;
  title: string;
  subtitle: string;
  status: string;
  timestamp: string;
  hash: string;
  amount?: string;
  from?: string | null;
  to?: string | null;
  network?: NetworkId | null;
  payload_hash?: string | null;
  signature?: string | null;
  fee?: string | null;
};

export type WalletSession = {
  has_wallet: boolean;
  locked: boolean;
  wallet_name: string | null;
  address: string | null;
  addresses?: Record<string, string> | null;
  assets: Asset[];
  activity: Activity[];
};

export type SignedTransaction = {
  from: string;
  to: string;
  symbol: string;
  amount: string;
  note: string;
  network: NetworkId;
  nonce: string;
  signedAt: string;
  payloadHash: string;
  signature: string;
  feeAmount: string;
  feeSymbol: string;
  totalDebit: string;
  postBalance: string;
  decimals: number;
  fiatValue: number;
  rawTx?: string | null;
  txHash?: string | null;
};

export type SendDraft = {
  to: string;
  symbol: string;
  network: NetworkId;
  amount: string;
  note: string;
};

export type SessionCommand = "create_wallet" | "import_wallet" | "unlock_wallet" | "send_transaction" | "swap_tokens" | "clear_wallet" | "refresh_prices";

export type View = "dashboard" | "send" | "receive" | "swap" | "assets" | "activity" | "security" | "settings";

export type QrResilience = "L" | "M" | "Q" | "H";

export type Toast = {
  id: number;
  message: string;
  tone: "info" | "success" | "warning" | "error";
  createdAt: number;
  duration: number;
  exiting: boolean;
};

type NetworkKind =
  | "bitcoin"
  | "evm"
  | "filecoin"
  | "injective"
  | "svm"
  | "tron"
  | "zcash";

type ChainVM =
  | "EVM"
  | "FVM"
  | "Multi-VM"
  | "SVM"
  | "TrVM"
  | null;

export type NetworkId =
  | "bitcoin"
  | "avalanche_c"
  | "bnb"
  | "ethereum"
  | "monad"
  | "arbitrum_one"
  | "base"
  | "optimism"
  | "polygon"
  | "filecoin"
  | "injective"
  | "solana"
  | "tron"
  | "zcash";


export interface NetworkConfig {
  vm_type: ChainVM;
  ticker: string;
  isL2: boolean;
  isTestNet: boolean;
  rpcUrl?: string;
}

export interface NetworkInstance {
  kind: NetworkKind;
  id: NetworkId;
  name: string;
  nickname?: string;
  chainId?: number;
  isL2?: boolean;
  isTestNet?: boolean;
  ticker?: string;
  vm_type?: ChainVM;
}

export type Network = NetworkConfig & NetworkInstance; // TODO: does it matter in which order they are put in?

export interface NetworkData {
  network_types: Record<string, NetworkConfig>;
  networks: NetworkInstance[];
}
