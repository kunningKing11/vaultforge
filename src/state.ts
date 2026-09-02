import { DEFAULT_NETWORK_ID, networks } from "./networks";
import type { ThemeName } from "./theme";
import type {
  Activity,
  Asset,
  FiatCurrency,
  NetworkId,
  QrResilience,
  SendDraft,
  SignedTransaction,
  View,
  WalletSession,
} from "./types";

export type WalletState =
  | { status: "missing" }
  | { status: "locked"; name: string }
  | {
      status: "unlocked";
      name: string;
      addresses: Record<string, string>;
      fiatCurrency: FiatCurrency;
      usdExchangeRate: number;
      assets: Asset[];
      activity: Activity[];
      enabledNetworks: NetworkId[];
      autoLockTimeoutSecs: number | null;
    };

type SetupWizardState = {
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
  appearance: ThemeName;
  fiatCurrency: FiatCurrency;
  enabledNetworks: NetworkId[];
  autoLockTimeoutSecs: number | null;
};

type AppState = {
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
    qrSvg: string;
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
  };
}

export const appState: AppState = {
  wallet: { status: "missing" },
  navigation: {
    currentView: "dashboard",
    selectedActivityId: "",
  },
  onboarding: {
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
    appearance: "vaultforge",
    fiatCurrency: "USD",
    enabledNetworks: networks.map((network) => network.id),
    autoLockTimeoutSecs: null,
  },
  send: {
    draft: emptySendDraft(),
    signedTransaction: null,
  },
  receive: {
    networkId: DEFAULT_NETWORK_ID,
    qrResilience: "M",
    qrSvg: "",
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
      const network = networks.find((candidate) => candidate.id === id);
      return network ? [network.id] : [];
    }),
    autoLockTimeoutSecs: session.auto_lock_timeout_secs,
  };
}

export function applyWalletSession(session: WalletSession): void {
  appState.wallet = walletStateFromSession(session);
  if (appState.wallet.status !== "unlocked") {
    resetSendFlow();
    appState.receive.qrSvg = "";
    appState.navigation.selectedActivityId = "";
  }
}

export function selectSetupFlow(flow: SetupWizardState["flow"]): void {
  appState.onboarding.flow = flow;
  appState.onboarding.step = 2;
  appState.onboarding.recoveryPhrase = "";
  appState.onboarding.recoveryPhraseVisible = false;
  appState.onboarding.acknowledgedBackup = false;
}

export function resetSendFlow(): void {
  appState.send.draft = emptySendDraft();
  appState.send.signedTransaction = null;
}

export function resetOnboarding(): void {
  const wizard = appState.onboarding;
  wizard.step = 1;
  wizard.flow = "create";
  wizard.name = "";
  wizard.walletPassword = "";
  wizard.confirmWalletPassword = "";
  wizard.walletPasswordVisible = false;
  wizard.recoveryPhrase = "";
  wizard.recoveryPhraseVisible = false;
  wizard.acknowledgedBackup = false;
  wizard.wordCount = 12;
  wizard.fiatCurrency = "USD";
  wizard.enabledNetworks = networks.map((network) => network.id);
  wizard.autoLockTimeoutSecs = null;
}
