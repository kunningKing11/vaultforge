import { copyFile, mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";

const targets = [
  { artifact: "linux-x64", platform: "linux-x86_64", formats: ["appimage", "deb", "rpm"] },
  { artifact: "linux-arm64", platform: "linux-aarch64", formats: ["appimage", "deb", "rpm"] },
  { artifact: "macos-x64", platform: "darwin-x86_64", formats: ["app"] },
  { artifact: "macos-arm64", platform: "darwin-aarch64", formats: ["app"] },
  { artifact: "windows-x64", platform: "windows-x86_64", formats: ["nsis"] },
  { artifact: "windows-arm64", platform: "windows-aarch64", formats: ["nsis"] },
] as const;

function format(file: string): string | null {
  if (file.endsWith(".app.tar.gz")) return "app";
  if (file.endsWith(".AppImage")) return "appimage";
  if (file.endsWith(".deb")) return "deb";
  if (file.endsWith(".rpm")) return "rpm";
  if (file.endsWith(".exe")) return "nsis";
  if (file.endsWith(".dmg")) return "dmg";
  return null;
}

async function filesIn(directory: string): Promise<string[]> {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = await Promise.all(
    entries.map((entry) =>
      entry.isDirectory()
        ? filesIn(join(directory, entry.name))
        : entry.isFile()
          ? [join(directory, entry.name)]
          : [],
    ),
  );
  return files.flat().sort();
}

/** Prepare a complete static Tauri updater manifest without publishing anything. */
export async function prepareReleaseAssets({
  artifacts,
  output,
  repository,
  tag,
  notes,
  pubDate = new Date().toISOString(),
}: {
  artifacts: string;
  output: string;
  repository: string;
  tag: string;
  notes: string;
  pubDate?: string;
}) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error("Invalid GitHub repository");
  if (!/^v?\d+\.\d+\.\d+(?:-[\da-zA-Z.-]+)?(?:\+[\da-zA-Z.-]+)?$/.test(tag))
    throw new Error("Invalid release version");
  const packages: { source: string; artifact: string; format: string }[] = [];
  for (const target of targets) {
    const directory = join(artifacts, `vaultforge-wallet-${target.artifact}`);
    for (const source of await filesIn(directory)) {
      const kind = format(source);
      if (kind) packages.push({ source, artifact: target.artifact, format: kind });
    }
  }
  // GitHub normalizes spaces and special characters in uploaded asset names.
  const fileName = (source: string) => basename(source).replace(/[^\w.-]/g, ".");
  const counts = new Map<string, number>();
  for (const pkg of packages)
    counts.set(fileName(pkg.source), (counts.get(fileName(pkg.source)) ?? 0) + 1);
  const assetName = (pkg: (typeof packages)[number]) =>
    counts.get(fileName(pkg.source))! > 1
      ? `${pkg.artifact}-${fileName(pkg.source)}`
      : fileName(pkg.source);
  const platforms: Record<string, { url: string; signature: string }> = {};
  const copies = new Map<string, string>();
  for (const pkg of packages) {
    const name = assetName(pkg);
    if (copies.has(name)) throw new Error(`Duplicate release asset: ${name}`);
    copies.set(name, pkg.source);
  }
  for (const target of targets) {
    for (const kind of target.formats) {
      const matches = packages.filter(
        (pkg) => pkg.artifact === target.artifact && pkg.format === kind,
      );
      if (matches.length !== 1)
        throw new Error(
          `Expected one ${kind} updater for ${target.artifact}, found ${matches.length}`,
        );
      const pkg = matches[0];
      const signaturePath = `${pkg.source}.sig`;
      const signature = (await readFile(signaturePath, "utf8")).trim();
      if (!signature) throw new Error(`Empty updater signature: ${signaturePath}`);
      const name = assetName(pkg);
      copies.set(`${name}.sig`, signaturePath);
      const entry = {
        url: `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`,
        signature,
      };
      platforms[`${target.platform}-${kind}`] = entry;
      // Older clients and unrecognized bundle types use OS-ARCH. Linux falls back to AppImage.
      if (kind === target.formats[0]) platforms[target.platform] = entry;
    }
  }
  const manifest = { version: tag.replace(/^v/, ""), notes, pub_date: pubDate, platforms };
  await mkdir(output, { recursive: true });
  for (const [name, source] of copies) await copyFile(source, join(output, name));
  await writeFile(join(output, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

if (import.meta.main) {
  const [artifacts, output, repository, tag, notesFile] = process.argv.slice(2);
  if (!artifacts || !output || !repository || !tag || !notesFile)
    throw new Error("Usage: prepare-release-assets.mts ARTIFACTS OUTPUT OWNER/REPO TAG NOTES_FILE");
  const manifest = await prepareReleaseAssets({
    artifacts,
    output,
    repository,
    tag,
    notes: await readFile(notesFile, "utf8"),
  });
  console.log(
    `Prepared latest.json for ${Object.keys(manifest.platforms).length} updater targets.`,
  );
}
