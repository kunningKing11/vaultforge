import { networks } from "./networks";
import { appState } from "./state";
import type { Activity, Network } from "./types";

export function unlockedWallet() {
  return appState.wallet.status === "unlocked" ? appState.wallet : null;
}

export function selectedNetwork(): Network {
  return networks.find((network) => network.id === appState.receive.networkId) ?? networks[0];
}

export function networkLabel(network: Network, includeChainId = false): string {
  if (network.kind === "evm" && includeChainId) {
    return `${network.ticker} - Chain ID ${network.chainId}`;
  }
  return network.ticker;
}

export function addressKeyForNetwork(network: Network): string {
  return network.addressKey;
}

export function addressForNetwork(network: Network): string {
  return unlockedWallet()?.addresses[addressKeyForNetwork(network)] ?? "";
}

export function receivePayload(): string {
  const network = selectedNetwork();
  const address = addressForNetwork(network);
  if (!address) return "";
  if (network.kind === "bitcoin") return `bitcoin:${address}`;
  if (network.kind === "evm") return `ethereum:${address}@${network.chainId}`;
  if (network.kind === "svm") return `solana:${address}`;
  return address;
}

export function selectedActivity(): Activity | null {
  const activity = unlockedWallet()?.activity ?? [];
  return (
    activity.find((item) => item.id === appState.navigation.selectedActivityId) ??
    activity[0] ??
    null
  );
}
