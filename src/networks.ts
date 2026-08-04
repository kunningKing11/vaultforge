import type { Network, NetworkAssetConfig, NetworkId, NetworkTokenConfig } from "./types";
import networkDataJson from "./networks.json";

type NetworkTemplate = Pick<Network, "vm_type" | "isL2" | "isTestNet">;
type NetworkDefinition = Pick<Network, "kind" | "id" | "name" | "addressKey" | "nativeAsset"> &
  Partial<Omit<Network, "kind" | "id" | "name" | "addressKey" | "nativeAsset" | "ticker">>;

export type NetworkDataSource = {
  schemaVersion: number;
  network_types: Record<Network["kind"], NetworkTemplate>;
  networks: NetworkDefinition[];
};

export type NormalizedNetworkRegistry = {
  schemaVersion: number;
  networks: Network[];
};

export type EvmNetwork = Network & {
  kind: "evm";
  chainId: number;
  vm_type: "EVM";
};

export const DEFAULT_NETWORK_ID: NetworkId = "ethereum";

function validateAsset(asset: NetworkAssetConfig, context: string) {
  if (!asset.symbol || !asset.name || !asset.coinGeckoId) {
    throw new Error(`${context} must define symbol, name, and coinGeckoId`);
  }
  if (!Number.isInteger(asset.decimals) || asset.decimals < 0) {
    throw new Error(`${context} has invalid decimals`);
  }
}

function validateToken(token: NetworkTokenConfig, context: string) {
  validateAsset(token, context);
  if (token.standard !== "erc20" || !/^0x[0-9a-fA-F]{40}$/.test(token.tokenAddress)) {
    throw new Error(`${context} must be an ERC-20 token with a valid contract address`);
  }
}

export function normalizeNetworkRegistry(source: NetworkDataSource): NormalizedNetworkRegistry {
  if (source.schemaVersion !== 1) throw new Error("Unsupported network registry schema version");

  const ids = new Set<string>();
  const networks = source.networks.map((definition) => {
    const template = source.network_types[definition.kind];
    if (!template) throw new Error(`Missing template for network kind ${definition.kind}`);
    if (ids.has(definition.id)) throw new Error(`Duplicate network id ${definition.id}`);
    ids.add(definition.id);

    const network = {
      ...template,
      ...definition,
      ticker: definition.nativeAsset.symbol,
      tokens: definition.tokens ?? [],
    } as Network;

    validateAsset(network.nativeAsset, `${network.id} native asset`);
    if (network.kind === "evm") {
      if (!network.chainId || !network.rpcUrl) {
        throw new Error(`${network.id} must define chainId and rpcUrl`);
      }
      const tokenSymbols = new Set<string>();
      const tokenAddresses = new Set<string>();
      for (const token of network.tokens) {
        validateToken(token, `${network.id} ${token.symbol}`);
        const address = token.tokenAddress.toLowerCase();
        if (tokenSymbols.has(token.symbol) || tokenAddresses.has(address)) {
          throw new Error(`${network.id} contains a duplicate token symbol or contract`);
        }
        tokenSymbols.add(token.symbol);
        tokenAddresses.add(address);
      }
    } else if (network.tokens.length > 0) {
      throw new Error(`${network.id} has configured tokens but no supported token standard`);
    }

    for (const url of [network.rpcUrl, network.apiUrl]) {
      if (url && !/^https:\/\//.test(url)) {
        throw new Error(`${network.id} has an invalid provider URL`);
      }
    }
    return network;
  });

  return { schemaVersion: source.schemaVersion, networks };
}

const networkRegistry = normalizeNetworkRegistry(networkDataJson as unknown as NetworkDataSource);

export function getNetworkRegistry(): NormalizedNetworkRegistry {
  return networkRegistry;
}

export function getNetworks(): Network[] {
  return networkRegistry.networks;
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
