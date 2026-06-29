import type { Activity, Network, NetworkId, QrResilience, SendDraft, SignedTransaction, View, WalletSession } from "./types";
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
  sendDraft: { to: "", symbol: "ETH", network: "ethereum", amount: "", note: "" } as SendDraft,
  selectedActivityId: "",
  busy: false,
  lockedDeleteStep: "idle" as "idle" | "confirm" | "countdown",
  lockedDeleteRemaining: 10,
  lockedDeleteTimer: null as number | null,
  pendingTxTimer: null as number | null,
};

export function selectedNetwork(): Network {
  return networks.find((n) => n.id === appState.receiveNetworkId) ?? networks[0];
}

export function networkDetail(network: Network, short = true): string {
  if (network.kind === "evm") return `${network.ticker}${short ? "" : ` - Chain ID ${network.chainId}`}`;
  if (network.kind === "bitcoin") return network.ticker;
  return network.ticker;
}

export function addressForNetwork(network: Network): string {
  const fallback = appState.session?.address ?? "";
  if (!fallback) return "";
  if (!appState.session?.addresses) return fallback;
  return appState.session.addresses[network.kind] ?? fallback;
}

export function receivePayload(): string {
  const net = selectedNetwork();
  const addr = addressForNetwork(net);
  if (!addr) return "";
  if (net.kind === "bitcoin") return `bitcoin:${addr}`;
  if (net.kind === "evm") return `ethereum:${addr}@${net.chainId}`;
  if (net.kind === "solana") return `solana:${addr}`;
  return addr;
}

export function selectedActivity(): Activity | null {
  const activity = appState.session?.activity ?? [];
  return activity.find((item) => item.id === appState.selectedActivityId) ?? activity[0] ?? null;
}
