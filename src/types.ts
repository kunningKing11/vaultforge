export type FiatCurrency = "USD" | "EUR" | "GBP" | "JPY";

export type TxStatus = {
  execution: "succeeded" | "failed";
  finality: "included" | "finalized";
  block_height: number | null;
  block_hash: string | null;
  confirmations: number | null;
  slot?: number | null;
};

export type AssetIdentity = {
  network: NetworkId;
  token_address: string | null;
};

export type Asset = AssetIdentity & {
  solana_token?: {
    program: string;
    extensions: number[];
    send_supported: boolean;
    swap_supported: boolean;
    unsupported_reason: string | null;
  } | null;
  symbol: string;
  unicode_symbol?: string | null;
  name: string;
  balance: string;
  decimals: number;
  price_usd: number;
  change_24h: number;
};

export type Activity = {
  asset?: AssetIdentity | null;
  counter_asset?: AssetIdentity | null;
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
  tx_status?: TxStatus | null;
};

export type WalletSession = {
  has_wallet: boolean;
  locked: boolean;
  wallet_name: string | null;
  addresses?: Record<string, string> | null;
  fiat_currency: FiatCurrency | null;
  usd_exchange_rate: number | null;
  assets: Asset[];
  activity: Activity[];
  enabled_networks: string[];
  auto_lock_timeout_secs: number | null;
  use_crypto_symbols: boolean;
  hide_below_usd: number;
  group_assets_by_chain: boolean;
};

export type CctpAttestationProgress = {
  session: WalletSession;
  state: "pending" | "available" | "waiting_source_finality" | "error";
  retry_after_secs: number;
  error: string | null;
};

export type RefreshWarning = {
  kind: "balance" | "value";
  subject: string;
};

export type WalletRefreshResult = {
  session: WalletSession;
  warnings: RefreshWarning[];
};

export type SolanaTokenSendReview = {
  tokenProgram: string;
  mintExtensions: number[];
  feeSchedule: { epoch: string; basisPoints: number; maximumFee: string } | null;
  sources: { address: string; amount: string; fee: string }[];
  transferFee: string;
  recipientAmount: string;
  accountRentLamports: string;
};

export type SignedTransaction = {
  solanaTokenSend?: SolanaTokenSendReview | null;
  tokenAddress: string | null;
  from: string;
  to: string;
  symbol: string;
  amount: string;
  note: string;
  destinationTag?: number | null;
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
  token_address: string | null;
  amount: string;
  note: string;
  destinationTag: number | null;
};

export type SessionCommand =
  | "create_wallet"
  | "import_wallet"
  | "unlock_wallet"
  | "update_wallet_settings"
  | "send_transaction"
  | "swap_tokens"
  | "clear_wallet"
  | "refresh_portfolio";

export type View =
  | "dashboard"
  | "send"
  | "receive"
  | "swap"
  | "assets"
  | "activity"
  | "security"
  | "settings";

export type QrResilience = "L" | "M" | "Q" | "H";

export type QrResilienceOption = {
  value: QrResilience;
  label: string;
  detail: string;
};

export type Toast = {
  id: number;
  message: string;
  tone: "info" | "success" | "warning" | "error";
  createdAt: number;
  duration: number;
  exiting: boolean;
};

type NetworkKind = "bitcoin" | "evm" | "filecoin" | "injective" | "svm" | "tron" | "xrpl" | "zcash";

export type TransactionStatusProvider = "bitcoin" | "evm" | "solana" | "tron" | "xrpl";

type ChainVM = "EVM" | "FVM" | "Multi-VM" | "SVM" | "TrVM" | null;

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
  | "xrpl"
  | "zcash";

export interface NetworkConfig {
  vm_type: ChainVM;
  ticker: string;
  isL2: boolean;
  isTestNet: boolean;
  transactionStatusProvider: TransactionStatusProvider | null;
  rpcUrl?: string;
  apiUrl?: string;
  addressKey: string;
  coinGeckoNetworkId?: string;
  nativeAsset: NetworkAssetConfig;
  tokens: NetworkTokenConfig[];
}

export interface NetworkAssetConfig {
  symbol: string;
  unicodeSymbol?: string;
  name: string;
  decimals: number;
  coinGeckoId: string;
}

export interface NetworkTokenConfig extends Omit<NetworkAssetConfig, "coinGeckoId"> {
  coinGeckoId?: string;
  standard: "erc20" | "spl" | "trc20";
  tokenAddress: string;
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

export type Network = NetworkConfig & NetworkInstance;

export type CctpPreparedStep = {
  operation_id: string;
  step: "approval" | "burn" | "mint" | "destination_setup";
  tx_hash: string;
  network: string;
  from: string;
  contract: string;
  recipient: string;
  amount_base_units: string;
  max_native_fee: string;
  context: {
    chain_id: number;
    from: string;
    to: string;
    value: string;
    data: string;
    nonce: number;
    gas_limit: number;
    max_fee_per_gas: string;
    max_priority_fee_per_gas: string;
    provisional_burn_fee: string | null;
    provisional_destination_fee: string;
  } | null;
  solana_context: {
    payer: string;
    blockhash: string;
    last_valid_height: number;
    fee_lamports: string;
    rent_lamports: string;
    event_account: string | null;
    fee_recipient: string | null;
    lookup: { address: string; addresses: string[] } | null;
    setup_slot: number | null;
    create_ata: boolean;
    provisional_destination_fee: string | null;
  } | null;
};
export type CctpState = {
  routes: {
    source: string;
    destination: string;
    source_usdc: string;
    destination_usdc: string;
    sender: string;
    destination_address: string;
    environment: "Mainnet" | "Sepolia";
    executable: boolean;
  }[];
  operations: {
    id: string;
    source: string;
    destination: string;
    recipient: string;
    amount_base_units: string;
    step: "Approval" | "Burn" | "AwaitingAttestation" | "Mint" | "Completed" | "Cancelled";
    progress: Activity;
    can_mint: boolean;
    last_error: string | null;
    needs_sync: boolean;
    prepared: CctpPreparedStep | null;
    attempt_step: string | null;
    attempt_status: string | null;
    attempt_error: string | null;
    attempt_hash: string | null;
  }[];
};

export type SwapRouteNode = {
  network: NetworkId;
  token_address: string | null;
  symbol: string;
  pool_fee: number | null;
  via: string | null;
};

export type RelayTerms = {
  destination_network: NetworkId;
  request_id: string;
  order_id: string;
  order: unknown;
  route: SwapRouteNode[];
  destination_hash: string | null;
  destination_status: TxStatus | null;
  status: string | null;
};

export type SolanaSwapToken = {
  mint: string;
  program: string;
  decimals: number;
  extensions: number[];
  fee_schedule: { epoch: string; basisPoints: number; maximumFee: string } | null;
  account_len: number;
};

export type ApiSwap = {
  solana_tokens?: { input: SolanaSwapToken; output: SolanaSwapToken } | null;
  delivery?: { signature: string; slot: number; net_output: string } | null;
  provider: "jupiter" | "lifi" | "relay";
  jupiter?: {
    order: {
      request_id: string;
      router: string;
      transaction_base64: string;
      quoted_output_base_units: string;
      minimum_output_base_units: string;
      raw_response: unknown;
    };
    message_hash: string;
    last_valid_height: number;
    deadline: number;
  } | null;
  request_id: string;
  route: SwapRouteNode[];
  transaction:
    | { family: "evm"; to: string; data: string; value: string }
    | {
        family: "solana";
        transaction: string;
        lookups: { address: string; addresses: string[] }[];
      };
};

export type UniswapQuote = {
  api?: ApiSwap | null;
  destination_network: NetworkId;
  relay: RelayTerms | null;
  route: SwapRouteNode[];
  id: string;
  network: NetworkId;
  token_in: string;
  token_out: string;
  sender: string;
  amount_base_units: string;
  quoted_output: string;
  minimum_received: string;
  slippage_bps: number;
  pool_fee: number | null;
  deadline: number;
  approval_required: boolean;
  approval_max_fee: string | null;
  swap_max_fee: string | null;
  input_symbol: string;
  output_symbol: string;
  input_decimals: number;
  output_decimals: number;
};

export type UniswapComparison = {
  quotes: UniswapQuote[];
  unavailable: { provider: string; error: string }[];
};

export type UniswapPreparedStep = {
  operation_id: string;
  step: "approval" | "swap";
  tx_hash: string;
  network: NetworkId;
  max_native_fee: string;
};

export type UniswapTerms = {
  api?: ApiSwap | null;
  relay?: RelayTerms | null;
  quote_id: string;
  chain_id: number | null;
  router: string;
  quoter: string;
  pool_fee: number | null;
  quoted_output: string;
  minimum_received: string;
  slippage_bps: number;
  deadline: number;
  approval_fee_budget: string | null;
  swap_fee_budget: string | null;
  input_symbol: string;
  output_symbol: string;
  input_decimals: number;
  output_decimals: number;
};

export type UniswapOperation = {
  id: string;
  network: NetworkId;
  token_in: string;
  token_out: string;
  amount_base_units: string;
  terms: UniswapTerms;
  progress: Activity;
  stage: "approve" | "swap" | "receive" | "done";
  terminal: boolean;
  needs_sync: boolean;
  can_cancel: boolean;
  can_prepare: boolean;
  prepared: UniswapPreparedStep | null;
  attempt_status: string | null;
  last_error: string | null;
};

export type UniswapState = {
  networks: NetworkId[];
  operations: UniswapOperation[];
};
