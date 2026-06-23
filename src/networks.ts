import type { EvmNetwork, Network, NetworkId } from "./types";
import networkData from "./networks.json";

export const DEFAULT_NETWORK_ID: NetworkId = "ethereum";

export const networks: Network[] = networkData as Network[];

export function normalizeNetworkId(value: string): NetworkId {
  return networks.find((network) => network.id === value)?.id ?? DEFAULT_NETWORK_ID;
}

export function networkById(id: string) {
  return networks.find((network) => network.id === id) ?? null;
}

export function networkDisplayName(id: string) {
  return networkById(id)?.name ?? id;
}

export function evmNetworks() {
  return networks.filter((network): network is EvmNetwork => network.kind === "evm");
}
