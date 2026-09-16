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
  test("normalizes the configured registry", () => {
    const registry = normalizeNetworkRegistry(networkSource());

    expect(registry.schemaVersion).toBe(1);
    expect(registry.networks.length).toBe(networkDataJson.networks.length);
    for (const network of registry.networks) {
      expect(network.ticker).toBe(network.nativeAsset.symbol);
      expect(network.tokens).toBeArray();
    }
    expect(
      registry.networks.find((network) => network.id === "bitcoin")!.nativeAsset.unicodeSymbol,
    ).toBe("₿");
  });

  test("rejects unsupported schemas and duplicate network ids", () => {
    const wrongSchema = networkSource();
    wrongSchema.schemaVersion = 2;
    expect(() => normalizeNetworkRegistry(wrongSchema)).toThrow(
      "Unsupported network registry schema version",
    );

    const duplicate = networkSource();
    duplicate.networks.push(structuredClone(duplicate.networks[0]!));
    expect(() => normalizeNetworkRegistry(duplicate)).toThrow("Duplicate network id bitcoin");
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
      "ethereum contains a duplicate token symbol or contract",
    );
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
      "ethereum contains a duplicate token symbol or contract",
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
});

test("unknown network ids fall back to Ethereum", () => {
  expect(normalizeNetworkId("not-a-network")).toBe(DEFAULT_NETWORK_ID);
});
