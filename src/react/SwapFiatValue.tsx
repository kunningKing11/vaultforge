import { Text } from "@coinbase/cds-web/typography";

import { money, usdToFiat, weiToNumber } from "../format";
import type { Asset, FiatCurrency } from "../types";

/** Display-only valuation. Signing and spend limits always retain integer base units. */
export function swapFiatValue(
  baseUnits: string,
  asset: Asset | undefined,
  currency: FiatCurrency,
  rate: number,
): string {
  if (
    !asset ||
    !Number.isFinite(asset.price_usd) ||
    asset.price_usd <= 0 ||
    !Number.isFinite(rate) ||
    rate <= 0 ||
    !/^\d+$/.test(baseUnits)
  )
    return "Price unavailable";
  const value = usdToFiat(weiToNumber(baseUnits, asset.decimals) * asset.price_usd, rate);
  return Number.isFinite(value) ? `≈ ${money(value, currency)}` : "Price unavailable";
}

export function SwapFiatValue({
  baseUnits,
  asset,
  currency,
  rate,
}: {
  baseUnits: string;
  asset: Asset | undefined;
  currency: FiatCurrency;
  rate: number;
}) {
  return (
    <Text font="caption" color="fgMuted">
      {swapFiatValue(baseUnits, asset, currency, rate)}
    </Text>
  );
}
