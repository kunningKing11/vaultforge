import { expect, test } from "bun:test";

import { assetGroupChains, assetGroups } from "../src/react/assetGroups";
import type { Asset } from "../src/types";

const ethUsdc: Asset = {
  network: "ethereum",
  token_address: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
  symbol: "USDC",
  name: "USD Coin",
  decimals: 6,
  balance: "600000",
  price_usd: 1,
  change_24h: 2,
};
const baseUsdc: Asset = {
  ...ethUsdc,
  network: "base",
  token_address: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
};

test("applies the hide threshold after grouping and preserves source balances", () => {
  const assets = [ethUsdc, baseUsdc];
  expect(assetGroups(assets, false, 1)).toHaveLength(0);
  const [group] = assetGroups(assets, true, 1);
  expect(group.asset.balance).toBe("1200000");
  expect(group.valueUsd).toBeCloseTo(1.2);
  expect(assetGroupChains(group).map((chain) => chain.asset.network)).toEqual(["base", "ethereum"]);
  expect(ethUsdc.balance).toBe("600000");
});

test("does not group unknown contracts merely because their symbols match", () => {
  expect(
    assetGroups(
      [ethUsdc, { ...baseUsdc, token_address: "0x1111111111111111111111111111111111111111" }],
      true,
      0,
    ),
  ).toHaveLength(2);
});

test("excludes zero-balance chains and represents the funded chain", () => {
  const [group] = assetGroups([{ ...ethUsdc, balance: "0" }, baseUsdc], true, 0);
  expect(group.asset.network).toBe("base");
  expect(group.members).toHaveLength(1);
  expect(assetGroupChains(group).map((chain) => chain.id)).toEqual(["base"]);
  const [empty] = assetGroups([{ ...ethUsdc, balance: "0" }], true, 0);
  expect(assetGroupChains(empty)).toHaveLength(0);
});

test("hides unpriced zero-balance selections when a cutoff is set", () => {
  const selection: Asset = {
    ...ethUsdc,
    network: "solana",
    token_address: "GcV9tEj62VncGithz4o4N9x6HWXARxuRgEAYk9zahNA8",
    symbol: "jlEURC",
    balance: "0",
    price_usd: 0,
  };
  expect(assetGroups([selection], false, 1)).toHaveLength(0);
  expect(assetGroups([selection], false, 0)).toHaveLength(1);
});

test("keeps Jupiter receipt tokens apart from their underlying assets", () => {
  const jupUsd: Asset = {
    ...ethUsdc,
    network: "solana",
    token_address: "JuprjznTrTSp2UFa3ZBUFgwdAmtZCq4MQCwysN55USD",
    symbol: "JupUSD",
  };
  const receipt: Asset = {
    ...jupUsd,
    token_address: "7GxATsNMnaC88vdwd2t3mwrFuQwwGvmYPrUQ4D6FotXk",
    symbol: "JUICED",
    price_usd: 1.04,
  };
  const groups = assetGroups([jupUsd, receipt], true, 0);
  expect(groups).toHaveLength(2);
  expect(groups.find((group) => group.asset.symbol === "JUICED")!.asset.price_usd).toBe(1.04);
});

test("keeps explicitly separate price identities apart", () => {
  const bridged = {
    ...ethUsdc,
    network: "polygon" as const,
    token_address: "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174",
  };
  expect(assetGroups([ethUsdc, bridged], true, 0)).toHaveLength(2);
});

test("keeps funded assets with unknown prices visible", () => {
  const unknown: Asset = {
    ...ethUsdc,
    network: "solana",
    token_address: "PEPEqnuuCDbBC89p1u9vpnP1KQ2oj1xTcQBsjt9X55m",
    symbol: "PEPE",
    balance: "1000000",
    price_usd: 0,
  };
  expect(assetGroups([unknown], false, 1)).toHaveLength(1);
});

test("requires distinct funded chains rather than multiple token entries", () => {
  const [group] = assetGroups([ethUsdc, baseUsdc], true, 0);
  const chains = assetGroupChains({
    ...group,
    members: [ethUsdc, { ...ethUsdc, balance: "1000000" }],
  });
  expect(chains).toHaveLength(1);
  expect(chains[0].asset.balance).toBe("1600000");
});

test("sorts chain breakdowns by descending value with stable alphabetical ties", () => {
  const arbitrumUsdc: Asset = {
    ...ethUsdc,
    network: "arbitrum_one",
    token_address: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
  };
  const [group] = assetGroups(
    [ethUsdc, arbitrumUsdc, { ...baseUsdc, balance: "1200000" }],
    true,
    0,
  );
  const chains = assetGroupChains(group);
  expect(chains.map((chain) => chain.id)).toEqual(["base", "arbitrum_one", "ethereum"]);
  expect(group.members.map((asset) => asset.network)).toEqual(["ethereum", "arbitrum_one", "base"]);
});

test("sums mixed-decimal LINK balances exactly without losing base units", () => {
  const link: Asset = {
    ...ethUsdc,
    symbol: "LINK",
    token_address: "0x514910771AF9Ca656af840dff83E8264EcF986CA",
    decimals: 18,
    balance: "1000000000000000001",
    price_usd: 10,
  };
  const solLink: Asset = {
    ...link,
    network: "solana",
    token_address: "LinkhB3afbBKb2EQQu7s7umdZceV3wcvAUJhQAfQ23L",
    decimals: 9,
    balance: "2000000001",
  };
  const [group] = assetGroups([link, solLink], true, 0);
  expect(group.asset.decimals).toBe(18);
  expect(group.asset.balance).toBe("3000000001000000001");
  expect(group.valueUsd).toBeCloseTo(30.00000001);
});
