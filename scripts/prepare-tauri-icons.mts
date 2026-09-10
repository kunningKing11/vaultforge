import { createHash } from "node:crypto";
import { readFile, rename, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

type HashedFile = {
  file: string;
  sha256: string;
};

type IconHashManifest = {
  version: 1;
  algorithm: "sha256";
  source: HashedFile;
  products: HashedFile[];
};

const androidDensities: string[] = ["hdpi", "mdpi", "xhdpi", "xxhdpi", "xxxhdpi"];
const androidLauncherFiles: string[] = [
  "ic_launcher.png",
  "ic_launcher_foreground.png",
  "ic_launcher_round.png",
];
const productFiles: string[] = [
  "32x32.png",
  "64x64.png",
  "128x128.png",
  "128x128@2x.png",
  "Square30x30Logo.png",
  "Square44x44Logo.png",
  "Square71x71Logo.png",
  "Square89x89Logo.png",
  "Square107x107Logo.png",
  "Square142x142Logo.png",
  "Square150x150Logo.png",
  "Square284x284Logo.png",
  "Square310x310Logo.png",
  "StoreLogo.png",
  "icon.icns",
  "icon.ico",
  "icon.png",
  "android/mipmap-anydpi-v26/ic_launcher.xml",
  ...androidDensities.flatMap((density) =>
    androidLauncherFiles.map((file) => `android/mipmap-${density}/${file}`),
  ),
  "android/values/ic_launcher_background.xml",
  "ios/AppIcon-20x20@1x.png",
  "ios/AppIcon-20x20@2x-1.png",
  "ios/AppIcon-20x20@2x.png",
  "ios/AppIcon-20x20@3x.png",
  "ios/AppIcon-29x29@1x.png",
  "ios/AppIcon-29x29@2x-1.png",
  "ios/AppIcon-29x29@2x.png",
  "ios/AppIcon-29x29@3x.png",
  "ios/AppIcon-40x40@1x.png",
  "ios/AppIcon-40x40@2x-1.png",
  "ios/AppIcon-40x40@2x.png",
  "ios/AppIcon-40x40@3x.png",
  "ios/AppIcon-60x60@2x.png",
  "ios/AppIcon-60x60@3x.png",
  "ios/AppIcon-76x76@1x.png",
  "ios/AppIcon-76x76@2x.png",
  "ios/AppIcon-83.5x83.5@2x.png",
  "ios/AppIcon-512@2x.png",
].sort();

const root: string = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const iconDirectory: string = resolve(root, "src-tauri", "icons");
const sourceFile: string = "icon.svg";
const sourcePath: string = resolve(iconDirectory, sourceFile);
const manifestPath: string = resolve(iconDirectory, "icon-hashes.json");

function isHashedFile(value: unknown): value is HashedFile {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<HashedFile>;
  return (
    typeof candidate.file === "string" &&
    typeof candidate.sha256 === "string" &&
    /^[a-f0-9]{64}$/.test(candidate.sha256)
  );
}

function isIconHashManifest(value: unknown): value is IconHashManifest {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<IconHashManifest>;
  return (
    candidate.version === 1 &&
    candidate.algorithm === "sha256" &&
    isHashedFile(candidate.source) &&
    Array.isArray(candidate.products) &&
    candidate.products.every(isHashedFile)
  );
}

async function sha256(path: string): Promise<string | null> {
  try {
    return createHash("sha256")
      .update(await readFile(path))
      .digest("hex");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw error;
  }
}

async function readManifest(): Promise<IconHashManifest | null> {
  try {
    const value: unknown = JSON.parse(await readFile(manifestPath, "utf8"));
    return isIconHashManifest(value) ? value : null;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT" || error instanceof SyntaxError) {
      return null;
    }
    throw error;
  }
}

async function cacheMissReason(sourceHash: string): Promise<string | null> {
  const manifest = await readManifest();
  if (!manifest) return "the hash manifest is missing or invalid";
  if (manifest.source.file !== sourceFile || manifest.source.sha256 !== sourceHash) {
    return "the source icon changed";
  }

  const productHashes = new Map(manifest.products.map((product) => [product.file, product.sha256]));
  if (
    manifest.products.length !== productFiles.length ||
    productFiles.some((file) => !productHashes.has(file))
  ) {
    return "the generated product list changed";
  }

  const actualHashes = await Promise.all(
    productFiles.map((file) => sha256(resolve(iconDirectory, file))),
  );
  for (const [index, file] of productFiles.entries()) {
    const actualHash = actualHashes[index];
    if (!actualHash) return `${file} is missing`;
    if (actualHash !== productHashes.get(file)) return `${file} changed`;
  }

  return null;
}

async function writeManifest(): Promise<void> {
  const sourceHash = await sha256(sourcePath);
  if (!sourceHash) throw new Error(`Icon source does not exist: ${sourcePath}`);

  const hashes = await Promise.all(
    productFiles.map(async (file): Promise<HashedFile> => {
      const hash = await sha256(resolve(iconDirectory, file));
      if (!hash) throw new Error(`Tauri did not generate ${file}`);
      return { file, sha256: hash };
    }),
  );
  const manifest: IconHashManifest = {
    version: 1,
    algorithm: "sha256",
    source: { file: sourceFile, sha256: sourceHash },
    products: hashes,
  };
  const temporaryPath = `${manifestPath}.${process.pid}.tmp`;
  await writeFile(temporaryPath, `${JSON.stringify(manifest, null, 2)}\n`);
  await rename(temporaryPath, manifestPath);
}

const sourceHash = await sha256(sourcePath);
if (!sourceHash) throw new Error(`Icon source does not exist: ${sourcePath}`);

const reason = await cacheMissReason(sourceHash);
if (!reason) {
  console.log("Tauri icons match icon-hashes.json; skipping generation");
  process.exit(0);
}

console.log(`Tauri icon cache miss: ${reason}`);
const generator = Bun.spawn(
  [process.execPath, "x", "tauri", "icon", sourcePath, "-o", iconDirectory],
  {
    cwd: root,
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
  },
);
const exitCode = await generator.exited;
if (exitCode !== 0) throw new Error(`Tauri icon generation failed with exit code ${exitCode}`);

await writeManifest();
console.log(`Generated ${productFiles.length} Tauri icons and updated icon-hashes.json`);
