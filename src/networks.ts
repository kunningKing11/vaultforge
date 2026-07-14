import type { Network, NetworkId } from "./types";
import networkDataJson from "./networks.json";

type NetworkTemplate = Omit<Network, "kind" | "id" | "name">;
type NetworkDefinition = Pick<Network, "kind" | "id" | "name"> & Partial<NetworkTemplate>;

type NetworkDataSource = {
  network_types: Record<Network["kind"], NetworkTemplate>;
  networks: NetworkDefinition[];
};

export type EvmNetwork = Network & {
  kind: "evm";
  chainId: number;
  vm_type: "EVM";
};

export const DEFAULT_NETWORK_ID: NetworkId = "ethereum";

const networkData = networkDataJson as unknown as NetworkDataSource;

export function getNetworks(): Network[] {
  return networkData.networks.map((network) => ({
    ...networkData.network_types[network.kind],
    ...network,
  }));
}

export const networks = getNetworks();

export function normalizeNetworkId(value: string): NetworkId {
  return networkById(value)?.id ?? DEFAULT_NETWORK_ID;
}

export function networkById(id: string) {
  return networks.find((network) => network.id === id) ?? null;
}

export function networkDisplayName(networkOrId: Network | string) {
  if (typeof networkOrId !== "string") return networkOrId.name;
  return networkById(networkOrId)?.name ?? networkOrId;
}

export function evmNetworks(): EvmNetwork[] {
  return networks.filter(
    (network): network is EvmNetwork =>
      network.kind === "evm" && network.vm_type === "EVM" && typeof network.chainId === "number",
  );
}
