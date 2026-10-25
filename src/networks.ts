import networkDataJson from "./networks.json";
import type {
  Network,
  NetworkAssetConfig,
  NetworkId,
  NetworkTokenConfig,
  TransactionStatusProvider,
} from "./types";

type NetworkTemplate = Pick<Network, "vm_type" | "isL2" | "isTestNet">;
type RequiredNetworkFields =
  | "kind"
  | "id"
  | "name"
  | "addressKey"
  | "nativeAsset"
  | "transactionStatusProvider";
type NetworkDefinition = Pick<Network, RequiredNetworkFields> &
  Partial<Omit<Network, RequiredNetworkFields | "ticker">>;

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

const transactionStatusProviderKinds = {
  bitcoin: "bitcoin",
  evm: "evm",
  solana: "svm",
  tron: "tron",
  xrpl: "xrpl",
} satisfies Record<TransactionStatusProvider, Network["kind"]>;

export const DEFAULT_NETWORK_ID: NetworkId = "ethereum";

function validateAsset(
  asset: NetworkAssetConfig | NetworkTokenConfig,
  context: string,
  requirePriceId = true,
) {
  if (!asset.symbol || !asset.name || (requirePriceId && !asset.coinGeckoId)) {
    throw new Error(`${context} must define symbol, name, and coinGeckoId`);
  }
  if (
    asset.coinGeckoId !== undefined &&
    (typeof asset.coinGeckoId !== "string" || !asset.coinGeckoId.trim())
  ) {
    throw new Error(`${context} has an invalid coinGeckoId`);
  }
  if (!Number.isInteger(asset.decimals) || asset.decimals < 0) {
    throw new Error(`${context} has invalid decimals`);
  }
  if (
    asset.unicodeSymbol !== undefined &&
    (!asset.unicodeSymbol.trim() || Array.from(asset.unicodeSymbol).length !== 1)
  ) {
    throw new Error(`${context} has an invalid Unicode symbol`);
  }
}

type TokenStandardDefinition = {
  networkKind: Network["kind"];
  isValidIdentifier(identifier: string): boolean;
  normalizeIdentifier(identifier: string): string;
};

const tokenStandardDefinitions = {
  erc20: {
    networkKind: "evm",
    isValidIdentifier: (identifier) => /^0x[0-9a-fA-F]{40}$/.test(identifier),
    normalizeIdentifier: (identifier) => identifier.toLowerCase(),
  },
  spl: {
    networkKind: "svm",
    isValidIdentifier: (identifier) => /^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(identifier),
    normalizeIdentifier: (identifier) => identifier,
  },
  trc20: {
    networkKind: "tron",
    isValidIdentifier: (identifier) => /^T[1-9A-HJ-NP-Za-km-z]{33}$/.test(identifier),
    normalizeIdentifier: (identifier) => identifier,
  },
} satisfies Record<NetworkTokenConfig["standard"], TokenStandardDefinition>;

function validateToken(token: NetworkTokenConfig, context: string, network: Network) {
  validateAsset(token, context, false);
  const definition = tokenStandardDefinitions[token.standard];
  if (
    definition.networkKind !== network.kind ||
    !definition.isValidIdentifier(token.tokenAddress)
  ) {
    throw new Error(
      `${context} has an invalid token standard or contract address on the ${network.id} network`,
    );
  }
}

function validateNetworkTokens(network: Network) {
  const tokenIdentifiers = new Set<string>();

  for (const token of network.tokens) {
    validateToken(token, `${network.id} ${token.symbol}`, network);
    const identifier = tokenStandardDefinitions[token.standard].normalizeIdentifier(
      token.tokenAddress,
    );
    if (tokenIdentifiers.has(identifier)) {
      throw new Error(`${network.id} contains a duplicate token contract`);
    }
    tokenIdentifiers.add(identifier);
  }
}

export function normalizeNetworkRegistry(source: NetworkDataSource): NormalizedNetworkRegistry {
  if (source.schemaVersion !== 2) throw new Error("Unsupported network registry schema version");

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

    if (definition.transactionStatusProvider === undefined) {
      throw new Error(`${network.id} must declare transactionStatusProvider`);
    }
    if (network.transactionStatusProvider !== null) {
      const provider = network.transactionStatusProvider;
      if (
        typeof provider !== "string" ||
        !Object.hasOwn(transactionStatusProviderKinds, provider) ||
        network.kind !== transactionStatusProviderKinds[provider]
      ) {
        throw new Error(
          `${network.id} has a transaction status provider for the wrong network kind`,
        );
      }
      const providerUrl =
        network.transactionStatusProvider === "bitcoin" ? network.apiUrl : network.rpcUrl;
      if (!providerUrl) {
        throw new Error(
          `${network.id} is missing the URL required by its transaction status provider`,
        );
      }
    }

    validateAsset(network.nativeAsset, `${network.id} native asset`);
    if (network.kind === "evm" && (!network.chainId || !network.rpcUrl)) {
      throw new Error(`${network.id} must define chainId and rpcUrl`);
    }
    validateNetworkTokens(network);

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

// Well-known block explorers only; unknown networks return null so callers
// render plain text instead of linking somewhere wrong.
const explorerBaseUrl: Record<string, string> = {
  arbitrum_one: "https://arbiscan.io/",
  arbitrum_sepolia: "https://sepolia.arbiscan.io/",
  avalanche_c: "https://snowtrace.io/",
  base: "https://basescan.org/",
  bitcoin: "https://blockstream.info/",
  bnb: "https://bscscan.com/",
  ethereum: "https://etherscan.io/",
  ethereum_sepolia: "https://sepolia.etherscan.io/",
  filecoin: "https://filfox.info/en/",
  injective: "https://explorer.injective.network/",
  monad: "https://monadvision.com/",
  optimism: "https://optimistic.etherscan.io/",
  polygon: "https://polygonscan.com/",
  solana: "https://solscan.io/",
  tron: "https://tronscan.org/#/",
  xrpl: "https://xrpscan.com/",
  zcash: "https://3xpl.com/zcash/",
};

const explorerPaths: Record<string, Partial<Record<"tx" | "address", string>>> = {
  filecoin: { tx: "message" },
  injective: { tx: "transaction", address: "account" },
  solana: { address: "account" },
  xrpl: { address: "account" },
  zcash: { tx: "transaction" },
};

export function explorerUrl(networkId: string, value: string, type: "tx" | "address") {
  const base = explorerBaseUrl[networkId];
  if (!base || !value) return null;
  const path = explorerPaths[networkId]?.[type] ?? type;
  return `${base}${path}/${value}`;
}

export function evmNetworks(): EvmNetwork[] {
  return networks.filter(
    (network): network is EvmNetwork =>
      network.kind === "evm" && network.vm_type === "EVM" && typeof network.chainId === "number",
  );
}
