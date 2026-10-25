import { describe, expect, test } from "bun:test";

import {
  DEFAULT_NETWORK_ID,
  type NetworkDataSource,
  normalizeNetworkId,
  normalizeNetworkRegistry,
} from "../src/networks";
import networkDataJson from "../src/networks.json";

function networkSource(): NetworkDataSource {
  return structuredClone(networkDataJson) as unknown as NetworkDataSource;
}

describe("network registry normalization", () => {
  test("allows a configured chain to opt out of transaction status", () => {
    const source = networkSource();
    source.networks.find((network) => network.id === "ethereum")!.transactionStatusProvider = null;
    const registry = normalizeNetworkRegistry(source);
    expect(
      registry.networks.find((network) => network.id === "ethereum")!.transactionStatusProvider,
    ).toBeNull();
  });

  test("allows metadata-only tokens while requiring native price IDs", () => {
    const source = networkSource();
    const solana = source.networks.find((network) => network.id === "solana")!;
    delete solana.tokens![0].coinGeckoId;
    expect(() => normalizeNetworkRegistry(source)).not.toThrow();
    Reflect.deleteProperty(solana.nativeAsset, "coinGeckoId");
    expect(() => normalizeNetworkRegistry(source)).toThrow(
      "solana native asset must define symbol, name, and coinGeckoId",
    );
  });

  test("allows same-symbol tokens with distinct contracts", () => {
    const source = networkSource();
    const ethereum = source.networks.find((network) => network.id === "ethereum")!;
    ethereum.tokens!.push({
      ...structuredClone(ethereum.tokens![0]!),
      tokenAddress: "0x1111111111111111111111111111111111111111",
    });
    expect(() => normalizeNetworkRegistry(source)).not.toThrow();
  });

  test("normalizes the configured registry", () => {
    const source = networkSource();
    const registry = normalizeNetworkRegistry(source);

    expect(registry.schemaVersion).toBe(2);
    expect(registry.networks.length).toBe(networkDataJson.networks.length);
    for (const network of registry.networks) {
      expect(network.ticker).toBe(network.nativeAsset.symbol);
      expect(network.tokens).toBeArray();
      expect(network.transactionStatusProvider).toBe(
        source.networks.find((definition) => definition.id === network.id)!
          .transactionStatusProvider,
      );
    }
    expect(
      registry.networks.find((network) => network.id === "bitcoin")!.nativeAsset.unicodeSymbol,
    ).toBe("₿");
  });

  test("normalizes token identifiers according to their standard", () => {
    const evmDuplicate = networkSource();
    const ethereum = evmDuplicate.networks.find((network) => network.id === "ethereum")!;
    ethereum.tokens!.push({
      ...structuredClone(ethereum.tokens![0]!),
      symbol: "USDC duplicate",
      tokenAddress: ethereum.tokens![0]!.tokenAddress.toLowerCase(),
    });
    expect(() => normalizeNetworkRegistry(evmDuplicate)).toThrow(
      "ethereum contains a duplicate token contract",
    );

    const solanaTokens = networkSource();
    const solana = solanaTokens.networks.find((network) => network.id === "solana")!;
    solana.tokens = [
      {
        standard: "spl",
        symbol: "Token One",
        name: "Token One",
        decimals: 9,
        tokenAddress: "So11111111111111111111111111111111111111112",
        coinGeckoId: "token-one",
      },
      {
        standard: "spl",
        symbol: "Token Two",
        name: "Token Two",
        decimals: 9,
        tokenAddress: "so11111111111111111111111111111111111111112",
        coinGeckoId: "token-two",
      },
    ];
    expect(() => normalizeNetworkRegistry(solanaTokens)).not.toThrow();
  });

  test("rejects empty optional CoinGecko IDs", () => {
    for (const id of ["", " "]) {
      const source = networkSource();
      const solana = source.networks.find((network) => network.id === "solana")!;
      solana.tokens![0].coinGeckoId = id;
      expect(() => normalizeNetworkRegistry(source)).toThrow("has an invalid coinGeckoId");
    }
  });

  test("rejects invalid providers and duplicate tokens", () => {
    const invalidProvider = networkSource();
    invalidProvider.networks.find((network) => network.id === "ethereum")!.rpcUrl =
      "http://localhost";
    expect(() => normalizeNetworkRegistry(invalidProvider)).toThrow(
      "ethereum has an invalid provider URL",
    );

    const duplicateToken = networkSource();
    const ethereum = duplicateToken.networks.find((network) => network.id === "ethereum")!;
    ethereum.tokens!.push(structuredClone(ethereum.tokens![0]!));
    expect(() => normalizeNetworkRegistry(duplicateToken)).toThrow(
      "ethereum contains a duplicate token contract",
    );
  });

  test("rejects invalid transaction status capabilities", () => {
    for (const provider of ["solana", "unknown"]) {
      const source = networkSource();
      Object.assign(
        source.networks.find((network) => network.id === "ethereum")!,
        {
          transactionStatusProvider: provider,
        },
      );
      expect(() => normalizeNetworkRegistry(source)).toThrow(
        "ethereum has a transaction status provider for the wrong network kind",
      );
    }
    const missingDeclaration = networkSource();
    Object.assign(missingDeclaration.networks[0]!, { transactionStatusProvider: undefined });
    expect(() => normalizeNetworkRegistry(missingDeclaration)).toThrow(
      "bitcoin must declare transactionStatusProvider",
    );

    for (const id of ["bitcoin", "solana"] as const) {
      const source = networkSource();
      const network = source.networks.find((item) => item.id === id)!;
      network.apiUrl = undefined;
      network.rpcUrl = undefined;
      expect(() => normalizeNetworkRegistry(source)).toThrow(
        `${id} is missing the URL required by its transaction status provider`,
      );
    }
  });

  test("rejects unsupported schemas and duplicate network ids", () => {
    const wrongSchema = networkSource();
    wrongSchema.schemaVersion = 1;
    expect(() => normalizeNetworkRegistry(wrongSchema)).toThrow(
      "Unsupported network registry schema version",
    );

    const duplicate = networkSource();
    duplicate.networks.push(structuredClone(duplicate.networks[0]!));
    expect(() => normalizeNetworkRegistry(duplicate)).toThrow("Duplicate network id bitcoin");
  });
});

test("unknown network ids fall back to Ethereum", () => {
  expect(normalizeNetworkId("not-a-network")).toBe(DEFAULT_NETWORK_ID);
});
