import type { ThemeName } from "./theme";
import type {
  Activity,
  Network,
  NetworkId,
  QrResilience,
  SendDraft,
  SignedTransaction,
  View,
  WalletSession,
} from "./types";
import { DEFAULT_NETWORK_ID, networks } from "./networks";

export const appState = {
  session: null as WalletSession | null,
  currentView: "dashboard" as View,
  receiveNetworkId: DEFAULT_NETWORK_ID as NetworkId,
  qrResilience: "M" as QrResilience,
  qrSvg: "",
  qrKey: "",
  qrGeneratingKey: "",
  signedTransaction: null as SignedTransaction | null,
  sendDraft: {
    to: "",
    symbol: "ETH",
    network: "ethereum",
    token_address: null,
    amount: "",
    note: "",
  } as SendDraft,
  selectedActivityId: "",
  busy: false,
  portfolioRefreshing: false,
  portfolioStale: false,
  portfolioRefreshId: 0,
  unlockPasswordVisible: false,
  lockedDeleteStep: "idle" as "idle" | "confirm" | "countdown",
  lockedDeleteRemaining: 10,
  lockedDeleteTimer: null as number | null,
  pendingTxTimer: null as number | null,
  setupWizard: {
    step: 1,
    flow: "create" as "create" | "import",
    name: "",
    walletPassword: "",
    confirmWalletPassword: "",
    walletPasswordVisible: false,
    mnemonic: "",
    generatedMnemonic: "",
    recoveryPhraseVisible: false,
    acknowledgedBackup: false,
    wordCount: 12 as 12 | 15 | 18 | 21 | 24,
    enabledNetworks: networks.map((n) => n.id) as string[],
    autoLockTimeoutSecs: null as number | null,
    appearance: "vaultforge" as ThemeName,
  },
  lastActivity: Date.now(),
  autoLockTimer: null as number | null,
};

export function selectedNetwork(): Network {
  return networks.find((n) => n.id === appState.receiveNetworkId) ?? networks[0];
}

export function networkDetail(network: Network, short = true): string {
  if (network.kind === "evm")
    return `${network.ticker}${short ? "" : ` - Chain ID ${network.chainId}`}`;
  if (network.kind === "bitcoin") return network.ticker;
  return network.ticker;
}

export function addressKeyForNetwork(network: Network): string {
  return network.kind === "svm" ? "solana" : network.kind;
}

export function addressForNetwork(network: Network): string {
  return appState.session?.addresses?.[addressKeyForNetwork(network)] ?? "";
}

export function receivePayload(): string {
  const network = selectedNetwork();
  const addr = addressForNetwork(network);
  if (!addr) return "";
  if (network.kind === "bitcoin") return `bitcoin:${addr}`;
  if (network.kind === "evm") return `ethereum:${addr}@${network.chainId}`;
  if (network.kind === "svm") return `solana:${addr}`;
  return addr;
}

export function selectedActivity(): Activity | null {
  const activity = appState.session?.activity ?? [];
  return activity.find((item) => item.id === appState.selectedActivityId) ?? activity[0] ?? null;
}
