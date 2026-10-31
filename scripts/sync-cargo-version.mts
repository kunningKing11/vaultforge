import { resolve } from "node:path";

type PackageJson = {
  version?: unknown;
};

const semverPattern: RegExp = /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/;

function isVersion(value: unknown): value is string {
  return typeof value === "string" && semverPattern.test(value);
}

const root: string = resolve(import.meta.dir, "..");
const packagePath: string = resolve(root, "package.json");
const cargoPath: string = resolve(root, "src-tauri", "Cargo.toml");

const packageJson: PackageJson = JSON.parse(await Bun.file(packagePath).text()) as PackageJson;
const version: unknown = packageJson.version;

if (!isVersion(version)) {
  throw new Error(`package.json has an invalid semver version: ${String(version)}`);
}

const cargoToml: string = await Bun.file(cargoPath).text();
const nextCargoToml: string = cargoToml.replace(
  /(^\[package\][\s\S]*?^version\s*=\s*")[^"]+("\s*$)/m,
  `$1${version}$2`,
);

if (nextCargoToml === cargoToml && !cargoToml.includes(`version = "${version}"`)) {
  throw new Error("Could not update src-tauri/Cargo.toml package version");
}

if (nextCargoToml !== cargoToml) {
  await Bun.write(cargoPath, nextCargoToml);
  console.log(`Synced src-tauri/Cargo.toml package version to ${version}`);
}
