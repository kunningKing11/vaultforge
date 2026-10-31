const compare = (left: string, right: string) => (left < right ? -1 : left > right ? 1 : 0);
const sortKeys = <T,>(record: Record<string, T>) =>
  Object.fromEntries(Object.entries(record).sort(([left], [right]) => compare(left, right)));

type IconManifest = {
  assets: Record<string, unknown>;
  chains: Record<string, string>;
  nativeCoins: Record<string, string>;
  tokens: { icon: string; network: string; tokenAddress: string }[];
};

/** Order icon IDs, network maps, and token groups deterministically. */
export function orderedIconManifest<T extends IconManifest>(manifest: T): T {
  return {
    ...manifest,
    assets: sortKeys(manifest.assets),
    chains: sortKeys(manifest.chains),
    nativeCoins: sortKeys(manifest.nativeCoins),
    tokens: [...manifest.tokens].sort(
      (left, right) =>
        compare(left.icon, right.icon) ||
        compare(left.network, right.network) ||
        compare(left.tokenAddress, right.tokenAddress),
    ),
  };
}

if (import.meta.main) {
  const args = Bun.argv.slice(2);
  if (args.length > 1 || args.some((arg) => arg !== "--check" && arg !== "--fix")) {
    throw new Error("Usage: lint-icon-order.mts [--check|--fix]");
  }
  const file = new URL("../src/icon-manifest.json", import.meta.url);
  const manifest = JSON.parse(await Bun.file(file).text()) as IconManifest;
  const ordered = orderedIconManifest(manifest);
  const sections = (["assets", "chains", "nativeCoins", "tokens"] as const).filter(
    (section) => JSON.stringify(manifest[section]) !== JSON.stringify(ordered[section]),
  );
  if (sections.length && args.includes("--fix")) {
    await Bun.write(file, `${JSON.stringify(ordered, null, 2)}\n`);
    console.log(`Sorted icon manifest: ${sections.join(", ")}`);
  } else if (sections.length) {
    console.error(
      `Icon manifest order is incorrect: ${sections.join(", ")}. Sort keys alphabetically; sort tokens by icon, then network, then contract. Run bun run lint:icon-order:fix.`,
    );
    process.exitCode = 1;
  } else {
    console.log("Icon manifest order passed.");
  }
}
