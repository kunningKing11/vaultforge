import manifest from "./icon-manifest.json";
import { assetKey } from "./react/model";
import type { AssetIdentity, NetworkId } from "./types";

const paths = new Map(
  Object.entries(manifest.assets).map(([id, asset]) => [id, `/icons/${asset.file}`]),
);
export const UNKNOWN_ASSET_ICON = paths.get(manifest.fallback)!;
const chains: Readonly<Record<string, string>> = manifest.chains;
const nativeCoins: Readonly<Record<string, string>> = manifest.nativeCoins;
const tokens = new Map(
  manifest.tokens.map((token) => [
    assetKey({ network: token.network as NetworkId, token_address: token.tokenAddress }),
    token.icon,
  ]),
);

/** Local official chain artwork, or the bundled question mark for unknown networks. */
export function chainIconPath(network: string): string {
  return paths.get(chains[network]) ?? UNKNOWN_ASSET_ICON;
}

/** Token identity is network + identifier; native coins use a separate mapping. */
export function assetIconPath(asset: AssetIdentity): string {
  const icon =
    asset.token_address == null ? nativeCoins[asset.network] : tokens.get(assetKey(asset));
  return (icon ? paths.get(icon) : undefined) ?? UNKNOWN_ASSET_ICON;
}
