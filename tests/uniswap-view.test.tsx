import { describe, expect, test } from "bun:test";

import { ThemeProvider } from "@coinbase/cds-web/system/ThemeProvider";
import { defaultTheme } from "@coinbase/cds-web/themes/defaultTheme";
import { renderToStaticMarkup } from "react-dom/server";

import { money } from "../src/format";
import { UniswapView } from "../src/react/UniswapView";
import type { UniswapController } from "../src/react/useUniswap";
import type { Asset, FiatCurrency, UniswapOperation, UniswapQuote } from "../src/types";

const assets: Asset[] = ["USDC", "OUT"].map((symbol, index) => ({
  network: "base",
  token_address: `0x${String(index + 1).repeat(40)}`,
  symbol,
  name: symbol,
  balance: "1000000",
  decimals: 6,
  price_usd: 1,
  change_24h: 0,
}));
const quote: UniswapQuote = {
  destination_network: "base",
  relay: null,
  route: [],
  id: "quote",
  network: "base",
  token_in: assets[0].token_address!,
  token_out: assets[1].token_address!,
  sender: "0xwallet",
  amount_base_units: "1000000",
  quoted_output: "2000000",
  minimum_received: "1990000",
  slippage_bps: 50,
  pool_fee: 500,
  deadline: 4_000_000_000,
  approval_required: true,
  approval_max_fee: "42000",
  swap_max_fee: null,
  input_symbol: "USDC",
  output_symbol: "OUT",
  input_decimals: 6,
  output_decimals: 6,
};
function controller(operations: UniswapOperation[] = []): UniswapController {
  return {
    data: { networks: ["base"], operations },
    quote,
    quotes: [quote],
    unavailable: [],
    selectQuote: () => {},
    review: null,
    error: null,
    clearQuote: () => {},
    requestQuote: async () => null,
    start: async () => null,
    prepare: async () => null,
    submit: async () => null,
    sync: async () => null,
    cancel: async () => null,
    load: async () => null,
    stop: () => {},
  };
}
function operation(): UniswapOperation {
  return {
    id: "saved",
    network: "base",
    token_in: quote.token_in,
    token_out: quote.token_out,
    amount_base_units: "1000000",
    terms: {
      quote_id: "quote",
      chain_id: 8453,
      router: "router",
      quoter: "quoter",
      pool_fee: 500,
      quoted_output: "2000000",
      minimum_received: "1990000",
      slippage_bps: 50,
      deadline: 4_000_000_000,
      approval_fee_budget: "42000",
      swap_fee_budget: null,
      input_symbol: "USDC",
      output_symbol: "OUT",
      input_decimals: 6,
      output_decimals: 6,
    },
    progress: {
      id: "saved",
      kind: "swap",
      title: "Swap pending",
      subtitle: "USDC to OUT",
      status: "pending",
      timestamp: "now",
      hash: "0xabc",
    },
    stage: "approve",
    terminal: false,
    needs_sync: true,
    can_cancel: false,
    can_prepare: false,
    prepared: null,
    attempt_status: "SubmissionUnknown",
    last_error: "Submission uncertain",
  };
}
function render(
  value: UniswapController,
  choices = assets,
  currency: FiatCurrency = "USD",
  rate = 1,
) {
  return renderToStaticMarkup(
    <ThemeProvider theme={defaultTheme} activeColorScheme="dark">
      <UniswapView
        assets={choices}
        busy={false}
        controller={value}
        fiatCurrency={currency}
        usdExchangeRate={rate}
      />
    </ThemeProvider>,
  );
}

describe("Uniswap screen", () => {
  test("shows Jupiter percentages and USD input-based estimates for each route and selected quote", () => {
    const choices = assets.map((asset, index) => ({
      ...asset,
      network: "solana" as const,
      token_address: `mint-${index}`,
      price_usd: index === 0 ? 2 : 9999,
    }));
    const jupiter = (bps: number): UniswapQuote => ({
      ...quote,
      id: `jupiter-${bps}`,
      network: "solana",
      destination_network: "solana",
      token_in: "mint-0",
      token_out: "mint-1",
      amount_base_units: "50000000",
      approval_required: false,
      approval_max_fee: null,
      swap_max_fee: "5000",
      api: {
        provider: "jupiter",
        jupiter: {
          order: {
            raw_response: {
              feeBps: bps,
              platformFee: { feeBps: bps, amount: "123456789", feeMint: "mint-1" },
            },
          },
        },
      } as NonNullable<UniswapQuote["api"]>,
    });
    const value = controller();
    value.data.networks = ["solana"];
    value.quote = jupiter(20);
    value.quotes = [value.quote, jupiter(50)];
    const html = render(value, choices, "EUR", 0.8);
    expect(html.match(/Jupiter platform fee: 0\.2%/g)?.length).toBe(2);
    expect(html).toContain("Jupiter platform fee: 0.5%");
    expect(html).toContain(`${money(0.2, "USD")} USD of initial input`);
    expect(html).toContain(`${money(0.5, "USD")} USD of initial input`);
  });
  test("uses a saved Jupiter operation's original input amount for the fee review", () => {
    const choices = assets.map((asset, index) => ({
      ...asset,
      network: "solana" as const,
      token_address: `mint-${index}`,
      price_usd: 2,
    }));
    const saved = operation();
    saved.network = "solana";
    saved.token_in = "mint-0";
    saved.token_out = "mint-1";
    saved.amount_base_units = "100000000";
    saved.terms.api = {
      provider: "jupiter",
      jupiter: {
        order: {
          raw_response: {
            feeBps: 20,
            platformFee: { feeBps: 20 },
          },
        },
      },
    } as NonNullable<UniswapQuote["api"]>;
    const value = controller([saved]);
    value.data.networks = ["solana"];
    const html = render(value, choices, "EUR", 0.8);
    expect(html).toContain("Jupiter platform fee: 0.2%");
    expect(html).toContain(`${money(0.4, "USD")} USD of initial input`);
    expect(html).toContain("included");
  });

  test("keeps an unconfirmed approval pending without offering another approval", () => {
    const saved = operation();
    saved.attempt_status = "Submitted";
    saved.last_error = null;
    saved.progress.title = "Swap approval pending";
    saved.progress.subtitle =
      "Waiting for the approval transaction receipt. The swap has not been sent.";
    const html = render(controller([saved]));
    expect(html).toContain(saved.progress.subtitle);
    expect(html).toContain("Step 2 of 4: Approve");
    expect(html).toContain("Check progress");
    expect(html).not.toContain("Prepare next transaction");
    expect(html).not.toContain("Confirm approval");
  });

  test("offers the swap after approval confirmation while finality polling continues", () => {
    const saved = operation();
    saved.attempt_status = "Submitted";
    saved.last_error = null;
    saved.stage = "swap";
    saved.can_prepare = true;
    saved.needs_sync = true;
    saved.progress.title = "Approval complete; prepare swap";
    saved.progress.subtitle =
      "Approval is complete. Choose Prepare next transaction to review its network fee, then confirm the swap.";
    const html = render(controller([saved]));
    expect(html).toContain(saved.progress.subtitle);
    expect(html).toContain("Step 3 of 4: Swap");
    expect(html).toContain("Prepare next transaction");
    expect(html).not.toContain("Confirm approval");
  });
  test("displays searchable network and asset pickers with icons", () => {
    const html = render(controller());
    expect(html).toContain('role="combobox"');
    expect(html).toContain("From asset");
    expect(html).toContain("To asset");
    expect(html).toContain("From network");
    expect(html).toContain("To network");
    expect(html).toContain("/icons/");
    expect(html).not.toContain("<select");
  });

  test("displays a saved uncertain transaction without offering cancellation", () => {
    const html = render(controller([operation()]));
    expect(html).toContain("Retry saved transaction");
    expect(html).toContain("Check progress");
    expect(html).not.toContain("Cancel swap");
    expect(html).not.toContain("Confirm swap");
  });
  test("displays approval fees separately and explains the later swap review", () => {
    const html = render(controller());
    expect(html).toContain("Maximum approval");
    expect(html).toContain("actual network fee");
    expect(html).toContain("Start approval");
    expect(html).not.toContain("Maximum swap");
  });
  test("offers Max beside the amount field", () => {
    const html = render(controller());
    expect(html).toContain("Amount to spend");
    expect(html).toMatch(/<button[^>]*>[\s\S]*?Max/);
  });
  test("offers every requested slippage preset and custom entry", () => {
    const html = render(controller());
    for (const label of [
      "Slippage",
      "0.1%",
      "0.2%",
      "0.5%",
      "1%",
      "Custom",
      "Refresh quote",
      "Updates every 15 seconds",
    ])
      expect(html).toContain(label);
    expect(html).toContain('aria-pressed="true"');
    expect(html).not.toContain("Maximum price change");
  });
  test("requires confirmation of a recovered signed swap", () => {
    const saved = operation();
    saved.attempt_status = "Signed";
    saved.needs_sync = false;
    saved.can_cancel = true;
    saved.prepared = {
      operation_id: "saved",
      step: "swap",
      tx_hash: "0xabc",
      network: "base",
      max_native_fee: "42000",
    };
    const html = render(controller([saved]));
    expect(html).toContain("Confirm swap");
    expect(html).toContain("Cancel swap");
    expect(html).not.toContain("Retry saved transaction");
  });
  test("values expected minimum and network fee using the wallet currency", () => {
    const priced = [
      ...assets.map((asset) => ({ ...asset, price_usd: asset.symbol === "OUT" ? 3 : 1 })),
      {
        network: "base" as const,
        token_address: null,
        symbol: "ETH",
        name: "Ether",
        balance: "0",
        decimals: 18,
        price_usd: 2000,
        change_24h: 0,
      },
    ];
    const value = controller();
    value.quote = { ...quote, approval_max_fee: "1000000000000000" };
    const html = render(value, priced, "EUR", 0.8);
    for (const amount of [0, 4.8, 4.776, 1.6]) expect(html).toContain(money(amount, "EUR"));
  });
});

describe("Swap stepper", () => {
  test("shows details before starting a swap", () => {
    expect(render(controller())).toContain("Step 1 of 4: Details");
  });
  test("shows destination settlement separately for a routed swap", () => {
    const saved = operation();
    saved.stage = "receive";
    saved.terms.relay = { route: [] } as unknown as NonNullable<UniswapQuote["relay"]>;
    const html = render(controller([saved]));
    expect(html).toContain("Step 4 of 5: Receive");
    expect(html).toContain('aria-label="Swap progress"');
  });
});

describe("Swap route comparison", () => {
  test("offers both providers with highest output labelled and fees separate", () => {
    const value = controller();
    value.quotes = [
      {
        ...quote,
        id: "better",
        quoted_output: "2100000",
        relay: {} as NonNullable<UniswapQuote["relay"]>,
      },
      quote,
    ];
    value.quote = value.quotes[0];
    const html = render(value);
    expect(html).toContain("Relay: 2.1 OUT");
    expect(html).toContain("Uniswap V3: 2.0 OUT");
    expect(html).toContain("Highest token output");
    expect(html).toContain('aria-pressed="true"');
    expect(html).toContain("Source network fees are separate");
  });
  test("shows a usable Relay quote when direct Uniswap fails", () => {
    const value = controller();
    value.quote = { ...quote, relay: {} as NonNullable<UniswapQuote["relay"]> };
    value.quotes = [value.quote];
    value.unavailable = [{ provider: "Uniswap V3", error: "No working pool found" }];
    const html = render(value);
    expect(html).toContain("Uniswap V3 unavailable: No working pool found");
    expect(html).toContain("Relay");
    expect(html).toContain("Start approval");
    expect(html).not.toContain('role="alert"');
  });
});

describe("LI.FI and Solana swap display", () => {
  test("displays LI.FI as a third route choice", () => {
    const value = controller();
    const relay = { ...quote, id: "relay", relay: {} as NonNullable<UniswapQuote["relay"]> };
    const lifi = {
      ...quote,
      id: "lifi",
      quoted_output: "2100000",
      api: { provider: "lifi" } as NonNullable<UniswapQuote["api"]>,
    };
    value.quotes = [lifi, relay, quote];
    value.quote = lifi;
    const html = render(value);
    expect(html).toContain("LI.FI: 2.1 OUT");
    expect(html).toContain("Relay: 2.0 OUT");
    expect(html).toContain("Uniswap V3: 2.0 OUT");
  });
  test("formats Solana fees in lamports rather than wei", () => {
    const value = controller();
    const choices = assets.map((asset) => ({ ...asset, network: "solana" as const }));
    value.data.networks = ["solana"];
    value.quote = {
      ...quote,
      network: "solana",
      destination_network: "solana",
      approval_required: false,
      approval_max_fee: null,
      swap_max_fee: "5000",
      api: { provider: "lifi" } as NonNullable<UniswapQuote["api"]>,
    };
    value.quotes = [value.quote];
    const html = render(value, choices);
    expect(html).toContain("0.000005 SOL");
    expect(html).toContain("LI.FI");
    expect(html).not.toContain("0.000000000000005");
  });
});
