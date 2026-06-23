import type { EvmNetwork, EvmNetworkInput, Network, NetworkId, NetworkInput } from "./types";

export const DEFAULT_NETWORK_ID: NetworkId = "ethereum";

const networkDefinitions: NetworkInput[] = [
  { kind: "evm", id: "ethereum", name: "Ethereum", chainId: 1, ticker: "ETH", vm_type: "EVM" },
  { kind: "evm", id: "monad", name: "Monad", chainId: 167004, ticker: "MON", vm_type: "EVM" },
  { kind: "evm", id: "polygon", name: "Polygon", chainId: 137, ticker: "MATIC", vm_type: "EVM", isL2: true },
  { kind: "evm", id: "arbitrum_one", name: "Arbitrum One", chainId: 42161, ticker: "ETH", vm_type: "EVM", isL2: true },
  { kind: "evm", id: "base", name: "Base", chainId: 8453, ticker: "ETH", vm_type: "EVM", isL2: true },
  { kind: "evm", id: "optimism", name: "Optimism", chainId: 10, ticker: "ETH", vm_type: "EVM", isL2: true },
  { kind: "evm", id: "avalanche_c", name: "Avalanche C-Chain", chainId: 43114, ticker: "AVAX", vm_type: "EVM" },
  { kind: "bitcoin", id: "bitcoin", name: "Bitcoin", ticker: "BTC" },
  { kind: "solana", id: "solana", name: "Solana", ticker: "SOL" },
  { kind: "zcash", id: "zcash", name: "Zcash", ticker: "ZEC" },
  { kind: "filecoin", id: "filecoin", name: "Filecoin", ticker: "FIL" },
  { kind: "injective", id: "injective", name: "Injective", ticker: "INJ" },
];

export const networks: Network[] = networkDefinitions.map(normalizeNetwork);

function normalizeNetwork(network: NetworkInput): Network {
  if (network.kind === "evm") {
    return {
      ...network,
      vm_type: (network as EvmNetworkInput).vm_type ?? "EVM",
      isL2: network.isL2 ?? false,
      isTestNet: network.isTestNet ?? false,
    };
  } else if (network.kind === "filecoin") {
    return {
      ...network,
      vm_type: "FVM",
      isL2: false,
      isTestNet: network.isTestNet ?? false,
    };
  } else if (network.kind === "injective") {
    return {
      ...network,
      vm_type: "MultiVM",
      isL2: false,
      isTestNet: network.isTestNet ?? false,
    };
  } else {
    return {
      ...network,
      isL2: false,
      isTestNet: network.isTestNet ?? false,
    }
  }
}

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
