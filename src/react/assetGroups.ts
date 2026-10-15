import { weiToNumber } from "../format";
import { networkById } from "../networks";
import type { Asset } from "../types";
import { assetKey } from "./model";

export type AssetGroup = { id: string; asset: Asset; members: Asset[]; valueUsd: number };

function registryPriceId(asset: Asset): string | undefined {
  const network = networkById(asset.network);
  if (!network) return undefined;
  if (asset.token_address == null) return network.nativeAsset.coinGeckoId;
  return network.tokens.find(
    (token) =>
      assetKey({ network: asset.network, token_address: token.tokenAddress }) === assetKey(asset),
  )?.coinGeckoId;
}

function aggregate(id: string, members: Asset[]): AssetGroup {
  if (members.length === 1)
    return {
      id,
      members,
      asset: members[0],
      valueUsd: weiToNumber(members[0].balance, members[0].decimals) * members[0].price_usd,
    };
  const decimals = Math.max(...members.map((asset) => asset.decimals));
  const balance = members
    .reduce(
      (total, asset) => total + BigInt(asset.balance) * 10n ** BigInt(decimals - asset.decimals),
      0n,
    )
    .toString();
  const valueUsd = members.reduce(
    (total, asset) => total + weiToNumber(asset.balance, asset.decimals) * asset.price_usd,
    0,
  );
  const units = weiToNumber(balance, decimals);
  const change =
    valueUsd > 0
      ? members.reduce(
          (total, asset) =>
            total + weiToNumber(asset.balance, asset.decimals) * asset.price_usd * asset.change_24h,
          0,
        ) / valueUsd
      : members[0].change_24h;
  return {
    id,
    members,
    valueUsd,
    asset: {
      ...members[0],
      balance,
      decimals,
      price_usd: units > 0 ? valueUsd / units : members[0].price_usd,
      change_24h: change,
    },
  };
}

/** Presentation only: registry IDs group known assets; unknown contracts stay separate. */
export function assetGroups(assets: Asset[], collapse: boolean, cutoff: number): AssetGroup[] {
  const groups = new Map<string, Asset[]>();
  for (const asset of assets) {
    const priceId = collapse ? registryPriceId(asset) : undefined;
    const id = priceId ? `price:${priceId}` : assetKey(asset);
    const members = groups.get(id) ?? [];
    members.push(asset);
    groups.set(id, members);
  }
  return [...groups]
    .map(([id, members]) => {
      const funded = members.filter((asset) => BigInt(asset.balance) > 0n);
      return aggregate(id, collapse && funded.length > 0 ? funded : members);
    })
    .filter(
      (group) => group.members.some((asset) => asset.price_usd <= 0) || group.valueUsd >= cutoff,
    )
    .sort((left, right) => right.valueUsd - left.valueUsd || left.id.localeCompare(right.id));
}

export function assetGroupChains(group: AssetGroup): AssetGroup[] {
  const chains = new Map<string, Asset[]>();
  for (const asset of group.members) {
    if (BigInt(asset.balance) === 0n) continue;
    const members = chains.get(asset.network) ?? [];
    members.push(asset);
    chains.set(asset.network, members);
  }
  return (
    [...chains]
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([id, members]) => aggregate(id, members))
      // Array.sort is stable: equal values retain the alphabetical chain order.
      .sort((left, right) => right.valueUsd - left.valueUsd)
  );
}
