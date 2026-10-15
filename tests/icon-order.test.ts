import { expect, test } from "bun:test";

import { orderedIconManifest } from "../scripts/lint-icon-order.mjs";

const fixture = () => ({
  assets: { usdc: { file: "usdc.svg" }, aave: { file: "aave.svg" } },
  chains: { polygon: "polygon", ethereum: "ethereum" },
  nativeCoins: { polygon: "polygon", ethereum: "ethereum" },
  tokens: [
    { icon: "usdc", network: "polygon", tokenAddress: "0x2" },
    { icon: "usdc", network: "ethereum", tokenAddress: "0x3" },
    { icon: "aave", network: "polygon", tokenAddress: "0x1" },
    { icon: "usdc", network: "ethereum", tokenAddress: "0x1" },
  ],
});

test("orders icon keys and token groups by icon, chain, and contract", () => {
  const ordered = orderedIconManifest(fixture());
  expect(Object.keys(ordered.assets)).toEqual(["aave", "usdc"]);
  expect(Object.keys(ordered.chains)).toEqual(["ethereum", "polygon"]);
  expect(Object.keys(ordered.nativeCoins)).toEqual(["ethereum", "polygon"]);
  expect(
    ordered.tokens.map(({ icon, network, tokenAddress }) => `${icon}:${network}:${tokenAddress}`),
  ).toEqual(["aave:polygon:0x1", "usdc:ethereum:0x1", "usdc:ethereum:0x3", "usdc:polygon:0x2"]);
});

test("preserves manifest data and does not mutate its input", () => {
  const original = fixture();
  const snapshot = JSON.stringify(original);
  const ordered = orderedIconManifest(original);
  expect(JSON.stringify(original)).toBe(snapshot);
  expect(ordered.assets.aave).toEqual(original.assets.aave);
  expect(ordered.tokens).toHaveLength(original.tokens.length);
  expect(JSON.stringify(orderedIconManifest(ordered))).toBe(JSON.stringify(ordered));
});
