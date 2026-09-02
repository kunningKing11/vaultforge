import { describe, expect, test } from "bun:test";

import networkDataJson from "../src/networks.json";
import {
  DEFAULT_NETWORK_ID,
  normalizeNetworkId,
  normalizeNetworkRegistry,
  type NetworkDataSource,
} from "../src/networks";

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
});

test("unknown network ids fall back to Ethereum", () => {
  expect(normalizeNetworkId("not-a-network")).toBe(DEFAULT_NETWORK_ID);
});
