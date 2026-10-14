import { DEFAULT_NETWORK_ID, networkById, networks } from "../networks";
import type {
  Activity,
  Asset,
  AssetIdentity,
  FiatCurrency,
  Network,
  NetworkId,
  QrResilience,
  SendDraft,
  SignedTransaction,
  TxStatus,
  View,
  WalletSession,
} from "../types";

export type ColorScheme = "light" | "dark";

export function assetKey(asset: AssetIdentity): string {
  const address = asset.token_address ?? null;
  const identifier =
    networkById(asset.network)?.kind === "evm" ? (address?.toLowerCase() ?? null) : address;
  return JSON.stringify([asset.network, identifier]);
}

export function findAsset(assets: readonly Asset[], identity: AssetIdentity): Asset | undefined {
  const key = assetKey(identity);
  return assets.find((asset) => assetKey(asset) === key);
}

export function isPendingTransactionActivity(item: Activity): boolean {
  return (
    item.kind === "send" &&
    Boolean(item.hash && item.network) &&
    networkById(item.network ?? "")?.transactionStatusProvider != null &&
    (item.tx_status
      ? item.tx_status.finality !== "finalized"
      : ["pending", "confirmed", "failed", "included"].includes(item.status))
  );
}

export function transactionStatusLabel(status: TxStatus): string {
  if (status.execution === "failed")
    return status.finality === "included" ? "failed (included)" : "failed";
  return status.finality === "finalized" ? "finalized" : "included";
}

function sameObservation(
  left: TxStatus | null | undefined,
  right: TxStatus | null | undefined,
): boolean {
  if (!left || !right) return !left && !right;
  return (
    left.execution === right.execution &&
    left.finality === right.finality &&
    left.block_height === right.block_height &&
    left.block_hash === right.block_hash &&
    left.confirmations === right.confirmations &&
    (left.slot ?? null) === (right.slot ?? null)
  );
}

export function applyTransactionObservations(
  activities: readonly Activity[],
  requests: readonly Activity[],
  observations: ReadonlyMap<string, TxStatus>,
  missingReceipts: ReadonlySet<string>,
): Activity[] {
  const requested = new Map(requests.map((item) => [item.id, item]));
  return activities.map((item) => {
    const request = requested.get(item.id);
    if (
      !request ||
      !isPendingTransactionActivity(item) ||
      request.hash !== item.hash ||
      request.network !== item.network ||
      !sameObservation(request.tx_status, item.tx_status)
    )
      return item;
    const observation = observations.get(item.id);
    if (observation)
      return { ...item, tx_status: observation, status: transactionStatusLabel(observation) };
    if (missingReceipts.has(item.id)) return { ...item, tx_status: null, status: "pending" };
    return item;
  });
}

export function transactionInclusionChanged(
  previous: TxStatus | null | undefined,
  next: TxStatus,
): boolean {
  return (
    !previous ||
    previous.execution !== next.execution ||
    previous.block_hash !== next.block_hash ||
    previous.block_height !== next.block_height ||
    (previous.slot ?? null) !== (next.slot ?? null)
  );
}

export type WalletState =
  | { status: "missing" }
  | { status: "locked"; name: string }
  | {
      status: "unlocked";
      name: string;
      addresses: Record<string, string>;
      fiatCurrency: FiatCurrency;
      usdExchangeRate: number;
      assets: WalletSession["assets"];
      activity: WalletSession["activity"];
      enabledNetworks: NetworkId[];
      autoLockTimeoutSecs: number | null;
      useCryptoSymbols: boolean;
      hideBelowUsd: number;
    };

export type SetupWizardState = {
  step: number;
  flow: "create" | "import";
  name: string;
  walletPassword: string;
  confirmWalletPassword: string;
  recoveryPhrase: string;
  bip39Passphrase: string;
  recoveryPhraseVisible: boolean;
  acknowledgedBackup: boolean;
  wordCount: 12 | 15 | 18 | 21 | 24;
  fiatCurrency: FiatCurrency;
  enabledNetworks: NetworkId[];
  autoLockTimeoutSecs: number | null;
  useCryptoSymbols: boolean;
};

export type AppState = {
  wallet: WalletState;
  navigation: {
    currentView: View;
    selectedActivityId: string;
  };
  onboarding: SetupWizardState;
  send: {
    draft: SendDraft;
    signedTransaction: SignedTransaction | null;
  };
  receive: {
    networkId: NetworkId;
    qrResilience: QrResilience;
  };
  portfolio: {
    status: "idle" | "refreshing" | "stale";
  };
  dialogs: {
    unlockPasswordVisible: boolean;
    deleteWallet: {
      step: "idle" | "confirm" | "countdown" | "deleting";
      secondsRemaining: number;
    };
  };
  operation: {
    busy: boolean;
  };
};

export function emptySendDraft(): SendDraft {
  return {
    to: "",
    symbol: "ETH",
    network: "ethereum",
    token_address: null,
    amount: "",
    note: "",
    destinationTag: null,
  };
}

export function sendAddressPlaceholder(networkId: NetworkId): string {
  const kind = networkById(networkId)?.kind;
  if (!kind) return "Recipient address";
  const placeholders: Record<Network["kind"], string> = {
    bitcoin: "bc1... / 1... / 3...",
    evm: "0x...",
    filecoin: "f1... / f3...",
    injective: "inj1...",
    svm: "Solana address (base58)",
    tron: "T...",
    xrpl: "r...",
    zcash: "t1... / t3...",
  };
  return placeholders[kind];
}

export function sendDraftForNetwork(
  draft: SendDraft,
  network: NetworkId,
  assets: readonly Asset[],
): SendDraft {
  const networkAssets = assets.filter((asset) => asset.network === network);
  if (draft.network === network && findAsset(networkAssets, draft)) return draft;
  const asset = networkAssets.find((candidate) => !candidate.token_address) ?? networkAssets[0];
  return {
    ...draft,
    network,
    symbol: asset?.symbol ?? networkById(network)?.nativeAsset.symbol ?? "",
    token_address: asset?.token_address ?? null,
    to: draft.network === network ? draft.to : "",
    amount: "",
    destinationTag: null,
  };
}

export function createSetupWizardState(): SetupWizardState {
  return {
    step: 1,
    flow: "create",
    name: "",
    walletPassword: "",
    confirmWalletPassword: "",
    recoveryPhrase: "",
    bip39Passphrase: "",
    recoveryPhraseVisible: false,
    acknowledgedBackup: false,
    wordCount: 12,
    fiatCurrency: "USD",
    enabledNetworks: networks.map((network) => network.id),
    autoLockTimeoutSecs: null,
    useCryptoSymbols: false,
  };
}

export function createInitialAppState(): AppState {
  return {
    wallet: { status: "missing" },
    navigation: {
      currentView: "dashboard",
      selectedActivityId: "",
    },
    onboarding: createSetupWizardState(),
    send: {
      draft: emptySendDraft(),
      signedTransaction: null,
    },
    receive: {
      networkId: DEFAULT_NETWORK_ID,
      qrResilience: "M",
    },
    portfolio: {
      status: "idle",
    },
    dialogs: {
      unlockPasswordVisible: false,
      deleteWallet: {
        step: "idle",
        secondsRemaining: 10,
      },
    },
    operation: {
      busy: false,
    },
  };
}

export function walletStateFromSession(session: WalletSession): WalletState {
  if (!session.has_wallet) return { status: "missing" };

  if (session.locked) {
    return {
      status: "locked",
      name: session.wallet_name ?? "Wallet",
    };
  }

  return {
    status: "unlocked",
    name: session.wallet_name ?? "Wallet",
    addresses: session.addresses ?? {},
    fiatCurrency: session.fiat_currency ?? "USD",
    usdExchangeRate: session.usd_exchange_rate ?? 1,
    assets: session.assets,
    activity: session.activity.map((item) =>
      item.kind === "send" && item.tx_status
        ? { ...item, status: transactionStatusLabel(item.tx_status) }
        : item,
    ),
    enabledNetworks: session.enabled_networks.flatMap((id) => {
      const network = networkById(id);
      return network ? [network.id] : [];
    }),
    autoLockTimeoutSecs: session.auto_lock_timeout_secs,
    useCryptoSymbols: session.use_crypto_symbols,
    hideBelowUsd: session.hide_below_usd ?? 1,
  };
}

export function applyWalletSession(state: AppState, session: WalletSession): AppState {
  const wallet = walletStateFromSession(session);
  const resetWalletUi = wallet.status !== "unlocked";
  const receiveNetworkId = receiveNetworkIdForWallet(state.receive.networkId, wallet);

  return {
    ...state,
    wallet,
    navigation: resetWalletUi
      ? { currentView: "dashboard", selectedActivityId: "" }
      : state.navigation,
    receive: {
      ...state.receive,
      networkId: receiveNetworkId,
    },
    send:
      wallet.status === "unlocked"
        ? {
            ...state.send,
            draft: sendDraftForNetwork(
              state.send.draft,
              wallet.enabledNetworks.includes(state.send.draft.network)
                ? state.send.draft.network
                : (wallet.enabledNetworks[0] ?? DEFAULT_NETWORK_ID),
              wallet.assets,
            ),
          }
        : { draft: emptySendDraft(), signedTransaction: null },
  };
}

function receiveNetworkIdForWallet(networkId: NetworkId, wallet: WalletState): NetworkId {
  if (wallet.status !== "unlocked") return networkId;
  if (wallet.enabledNetworks.includes(networkId)) return networkId;
  return wallet.enabledNetworks[0] ?? DEFAULT_NETWORK_ID;
}

export function networkLabel(
  network: Network,
  useCryptoSymbols: boolean,
  includeChainId = false,
): string {
  const asset = network.nativeAsset;
  const symbol = useCryptoSymbols && asset.unicodeSymbol ? asset.unicodeSymbol : asset.symbol;
  return network.kind === "evm" && includeChainId
    ? `${symbol} - Chain ID ${network.chainId}`
    : symbol;
}

export function addressForNetwork(wallet: WalletState, network: Network): string {
  if (wallet.status !== "unlocked") return "";
  return wallet.addresses[network.addressKey] ?? "";
}

export function receivePayload(wallet: WalletState, network: Network): string {
  const address = addressForNetwork(wallet, network);
  if (!address) return "";
  if (network.kind === "bitcoin") return `bitcoin:${address}`;
  if (network.kind === "evm") return `ethereum:${address}@${network.chainId}`;
  if (network.kind === "svm") return `solana:${address}`;
  return address;
}

export function selectedActivity(wallet: WalletState, selectedActivityId: string): Activity | null {
  if (wallet.status !== "unlocked") return null;
  return (
    wallet.activity.find((item) => item.id === selectedActivityId) ?? wallet.activity[0] ?? null
  );
}

/** Orders display-only asset views by their current fiat value, then stable identifiers. */
export function sortAssetsByValue(assets: ReadonlyArray<Asset>): Asset[] {
  return [...assets].sort((left, right) => {
    const valueDifference = displayAssetValueUsd(right) - displayAssetValueUsd(left);
    if (Number.isFinite(valueDifference) && valueDifference !== 0) return valueDifference;

    const symbolDifference = left.symbol.localeCompare(right.symbol);
    if (symbolDifference !== 0) return symbolDifference;

    const networkDifference = left.network.localeCompare(right.network);
    if (networkDifference !== 0) return networkDifference;

    return (left.token_address ?? "").localeCompare(right.token_address ?? "");
  });
}

export function sortAssetsForSelection(assets: readonly Asset[]): Asset[] {
  return [...assets].sort((left, right) => {
    const fundedDifference = Number(BigInt(right.balance) > 0n) - Number(BigInt(left.balance) > 0n);
    if (fundedDifference !== 0) return fundedDifference;
    return (
      left.symbol.localeCompare(right.symbol, "en", { sensitivity: "base" }) ||
      left.name.localeCompare(right.name, "en", { sensitivity: "base" }) ||
      assetKey(left).localeCompare(assetKey(right), "en")
    );
  });
}

export function sortNetworksForSelection(
  networkIds: readonly NetworkId[],
  assets: readonly Asset[],
): Network[] {
  const fundedNetworks = new Set(
    assets.filter((asset) => BigInt(asset.balance) > 0n).map((asset) => asset.network),
  );
  return networkIds
    .flatMap((id) => {
      const network = networkById(id);
      return network ? [network] : [];
    })
    .sort((left, right) => {
      const fundedDifference =
        Number(fundedNetworks.has(right.id)) - Number(fundedNetworks.has(left.id));
      return (
        fundedDifference ||
        left.name.localeCompare(right.name, "en", { sensitivity: "base" }) ||
        left.id.localeCompare(right.id, "en")
      );
    });
}

function displayAssetValueUsd(asset: Asset): number {
  return (Number(asset.balance) / 10 ** asset.decimals) * asset.price_usd;
}
