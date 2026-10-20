import { describe, expect, test } from "bun:test";

import { ThemeProvider } from "@coinbase/cds-web/system/ThemeProvider";
import { defaultTheme } from "@coinbase/cds-web/themes/defaultTheme";
import { renderToStaticMarkup } from "react-dom/server";

import { UNKNOWN_ASSET_ICON } from "../src/assetIcons";
import { SlippagePicker } from "../src/react/SlippagePicker";
import { SwapRoutePreview } from "../src/react/SwapRoutePreview";
import type { SwapRouteNode } from "../src/types";

const route: SwapRouteNode[] = [
  {
    network: "base",
    token_address: "0x1111111111111111111111111111111111111111",
    symbol: "UNKNOWN",
    pool_fee: 500,
    via: "Uniswap V3",
  },
  { network: "base", token_address: null, symbol: "ETH", pool_fee: null, via: "Relay bridge" },
  { network: "optimism", token_address: null, symbol: "ETH", pool_fee: null, via: "Swap" },
];
function render(value: React.ReactNode) {
  return renderToStaticMarkup(
    <ThemeProvider theme={defaultTheme} activeColorScheme="dark">
      {value}
    </ThemeProvider>,
  );
}
describe("Swap route preview", () => {
  test("displays multi-step icons chain badges arrows and reported pool fees", () => {
    const html = render(<SwapRoutePreview route={route} />);
    expect(html).toContain('aria-label="Swap route"');
    expect(html.match(/role="listitem"/g)).toHaveLength(3);
    expect(html.match(/→/g)).toHaveLength(2);
    expect(html).toContain("0.05% pool fee");
    expect(html).toContain("Base network");
    expect(html).toContain("Optimism network");
    expect(html).toContain(UNKNOWN_ASSET_ICON);
    expect(html).toContain("fee unavailable");
  });
  test("does not invent undisclosed fees or a route before a quote", () => {
    expect(render(<SwapRoutePreview route={[]} />)).not.toContain("Swap route");
    const html = render(
      <SwapRoutePreview route={route.map((node) => ({ ...node, pool_fee: null }))} />,
    );
    expect(html).not.toContain("% pool fee");
  });
  test("shows the custom percentage field only when custom is selected", () => {
    const props = { custom: "0.35", disabled: false, onChange: () => {}, onCustomChange: () => {} };
    expect(render(<SlippagePicker {...props} value="Custom" />)).toContain('value="0.35"');
    expect(render(<SlippagePicker {...props} value="0.5" />)).not.toContain("Custom slippage (%)");
  });
});
