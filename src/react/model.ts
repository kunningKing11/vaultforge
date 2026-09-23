import { DEFAULT_NETWORK_ID, networkById, networks } from "../networks";
import type {
  Activity,
  Asset,
  FiatCurrency,
  Network,
  NetworkId,
  QrResilience,
  SendDraft,
  SignedTransaction,
  View,
  WalletSession,
} from "../types";

export type ColorScheme = "light" | "dark";

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
    };

export type SetupWizardState = {
  step: number;
  flow: "create" | "import";
  name: string;
  walletPassword: string;
  confirmWalletPassword: string;
  walletPasswordVisible: boolean;
  recoveryPhrase: string;
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
      step: "idle" | "confirm" | "countdown";
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

export function createSetupWizardState(): SetupWizardState {
  return {
    step: 1,
    flow: "create",
    name: "",
    walletPassword: "",
    confirmWalletPassword: "",
    walletPasswordVisible: false,
    recoveryPhrase: "",
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
    activity: session.activity,
    enabledNetworks: session.enabled_networks.flatMap((id) => {
      const network = networkById(id);
      return network ? [network.id] : [];
    }),
    autoLockTimeoutSecs: session.auto_lock_timeout_secs,
    useCryptoSymbols: session.use_crypto_symbols,
  };
}

export function applyWalletSession(state: AppState, session: WalletSession): AppState {
  let wallet = walletStateFromSession(session);
  if (wallet.status === "unlocked" && state.wallet.status === "unlocked") {
    const settled = new Map(
      state.wallet.activity
        .filter((item) => item.status === "confirmed" || item.status === "failed")
        .map((item) => [item.id, item]),
    );
    wallet = {
      ...wallet,
      activity: wallet.activity.map((item) => {
        const previous = settled.get(item.id);
        return item.status === "pending" &&
          previous?.hash === item.hash &&
          previous.network === item.network
          ? { ...item, status: previous.status }
          : item;
      }),
    };
  }
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
    send: resetWalletUi ? { draft: emptySendDraft(), signedTransaction: null } : state.send,
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

function displayAssetValueUsd(asset: Asset): number {
  return (Number(asset.balance) / 10 ** asset.decimals) * asset.price_usd;
}
