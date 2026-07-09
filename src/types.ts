export type Asset = {
  symbol: string;
  name: string;
  balance: string;
  decimals: number;
  price_usd: number;
  change_24h: number;
  network: string;
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
  network?: string | null;
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
  network: string;
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
  network: string;
  amount: string;
  note: string;
};

export type SessionCommand = "create_wallet" | "import_wallet" | "unlock_wallet" | "send_transaction" | "swap_tokens" | "clear_wallet" | "refresh_prices";

export type View = "dashboard" | "send" | "receive" | "swap" | "assets" | "activity" | "security" | "settings";

export type QrResilience = "L" | "M" | "Q" | "H";

export type NetworkId =
  | "ethereum"
  | "monad"
  | "polygon"
  | "arbitrum_one"
  | "base"
  | "optimism"
  | "avalanche_c"
  | "bitcoin"
  | "solana"
  | "zcash"
  | "filecoin"
  | "injective";

export type Toast = {
  id: number;
  message: string;
  tone: "info" | "success" | "warning" | "error";
  createdAt: number;
  duration: number;
  exiting: boolean;
};

export type EvmNetwork = {
  kind: "evm";
  id: NetworkId;
  name: string;
  chainId: number;
  ticker: string;
  vm_type?: "EVM";
  isL2: boolean;
  isTestNet: boolean;
};

export type BitcoinNetwork = {
  kind: "bitcoin";
  id: NetworkId;
  name: string;
  ticker: "BTC";
  isL2: boolean,
  isTestNet: boolean,
}

export type LightningNetwork = {
  kind: "lightning";
  id: NetworkId;
  name: string;
  ticker: "BTC";
  isL2: boolean,
  isTestNet: boolean,
}

export type SolanaNetwork = {
  kind: "solana";
  id: NetworkId;
  name: string;
  ticker: "SOL";
  vm_type: "SVM",
  isL2: boolean,
  isTestNet: boolean,
}

export type ZcashNetwork = {
  kind: "zcash";
  id: NetworkId;
  name: string;
  ticker: "ZEC";
  isL2: boolean;
  isTestNet: boolean;
}

export type FilecoinNetwork = {
  kind: "filecoin";
  id: NetworkId;
  name: string;
  ticker: "FIL";
  vm_type: "FVM",
  isL2: boolean,
  isTestNet: boolean,
}

export type InjectiveNetwork = {
  kind: "injective";
  id: NetworkId;
  name: string;
  ticker: "INJ";
  vm_type: "MultiVM",
  isL2: boolean,
  isTestNet: boolean,
}

export type Network =
  | EvmNetwork
  | BitcoinNetwork
  | LightningNetwork
  | SolanaNetwork
  | ZcashNetwork
  | FilecoinNetwork
  | InjectiveNetwork;
