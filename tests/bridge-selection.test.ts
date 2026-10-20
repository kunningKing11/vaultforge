import { describe, expect, test } from "bun:test";

import { bridgeSelection } from "../src/react/bridgeSelection";
import type { Asset, CctpState } from "../src/types";

function route(
  source: string,
  destination: string,
  executable = true,
): CctpState["routes"][number] {
  return {
    source,
    destination,
    executable,
    source_usdc: `native-${source}`,
    destination_usdc: `native-${destination}`,
    sender: "sender",
    destination_address: "recipient",
    environment: "Mainnet",
  };
}
const routes = [
  route("base", "ethereum"),
  route("ethereum", "base"),
  route("ethereum", "arbitrum_one"),
];
const assets: Asset[] = [
  {
    network: "base",
    token_address: "bridged-usdc",
    symbol: "USDC",
    name: "Bridged USDC",
    balance: "1000000",
    decimals: 6,
    price_usd: 1,
    change_24h: 0,
  },
];

describe("cross-chain selection", () => {
  test("binds asset identities to the route contracts rather than a matching ticker", () => {
    const selected = bridgeSelection(routes, assets, ["base", "ethereum"], "base", "ethereum");
    expect(selected.route?.source_usdc).toBe("native-base");
    expect(selected.route?.source_usdc).not.toBe(assets[0].token_address!);
    expect(selected.route?.destination_usdc).toBe("native-ethereum");
  });
  test("defaults to funded networks and uses those same defaults for execution", () => {
    const selected = bridgeSelection(
      routes,
      assets,
      ["base", "ethereum", "arbitrum_one"],
      "ethereum_sepolia",
      "invalid",
    );
    expect(selected.source).toBe("base");
    expect(selected.destination).toBe("ethereum");
    expect(selected.route?.source).toBe(selected.source);
    expect(selected.route?.destination).toBe(selected.destination);
  });
  test("filters gated unknown and disabled networks", () => {
    const selected = bridgeSelection(
      [...routes, route("solana", "base", false), route("unknown", "base")],
      assets,
      ["base", "ethereum"],
      "",
      "",
    );
    expect(selected.sourceNetworks).toEqual(["base", "ethereum"]);
    expect(selected.destinationNetworks).toEqual(["ethereum"]);
    const unavailable = bridgeSelection(routes, assets, ["base"], "", "");
    expect(unavailable.route).toBeUndefined();
    expect(unavailable.sourceNetworks).toEqual([]);
  });
  test("replaces an invalid destination after the source changes", () => {
    const selected = bridgeSelection(
      routes,
      assets,
      ["base", "ethereum", "arbitrum_one"],
      "ethereum",
      "ethereum",
    );
    expect(selected.destination).not.toBe("ethereum");
    expect(selected.route?.destination).toBe(selected.destination);
    expect(selected.destinationNetworks).toContain("base");
    expect(selected.route?.source).toBe("ethereum");
  });
});
