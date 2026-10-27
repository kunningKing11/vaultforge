import { expect, test } from "bun:test";
import { access, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { prepareReleaseAssets } from "../scripts/prepare-release-assets.mjs";

async function fixture(
  run: (options: Parameters<typeof prepareReleaseAssets>[0]) => Promise<void>,
) {
  const root = await mkdtemp(join(tmpdir(), "vaultforge-release-test-"));
  const options = {
    artifacts: join(root, "artifacts"),
    output: join(root, "release"),
    repository: "owner/wallet",
    tag: "v0.5.0",
    notes: "Release notes",
    pubDate: "2026-10-16T00:00:00Z",
  };
  try {
    for (const [target, extensions] of Object.entries({
      "linux-x64": ["AppImage", "deb", "rpm"],
      "linux-arm64": ["AppImage", "deb", "rpm"],
      "macos-arm64": ["app.tar.gz"],
      "windows-x64": ["exe"],
      "windows-arm64": ["exe"],
    })) {
      const directory = join(options.artifacts, `vaultforge-wallet-${target}`, "bundle");
      await mkdir(directory, { recursive: true });
      for (const extension of extensions) {
        // Identical filenames exercise architecture disambiguation and GitHub name normalization.
        const file = join(directory, `VaultForge Wallet.${extension}`);
        await writeFile(file, `fixture package for ${target}`);
        await writeFile(`${file}.sig`, `fixture-signature-${target}-${extension}\n`);
      }
      if (target.startsWith("macos"))
        await writeFile(join(directory, "VaultForge Wallet.dmg"), "manual installer");
    }
    await run(options);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test("builds complete updater manifests with matching copied packages and signatures", async () => {
  await fixture(async (options) => {
    const manifest = await prepareReleaseAssets(options);
    expect(manifest.version).toBe("0.5.0");
    expect(manifest.notes).toBe(options.notes);
    expect(Object.keys(manifest.platforms)).toHaveLength(14);
    expect(manifest.platforms["linux-x86_64-deb"].url.endsWith(".deb")).toBe(true);
    expect(manifest.platforms["linux-x86_64-rpm"].url.endsWith(".rpm")).toBe(true);
    expect(manifest.platforms["linux-x86_64"].url.endsWith(".AppImage")).toBe(true);
    expect(manifest.platforms).not.toHaveProperty("darwin-x86_64");
    for (const entry of Object.values(manifest.platforms)) {
      expect(
        entry.url.startsWith("https://github.com/owner/wallet/releases/download/v0.5.0/"),
      ).toBe(true);
      expect(entry.url).not.toContain(" ");
      const name = decodeURIComponent(new URL(entry.url).pathname.split("/").at(-1)!);
      await access(join(options.output, name));
      expect((await readFile(join(options.output, `${name}.sig`), "utf8")).trim()).toBe(
        entry.signature,
      );
    }
    expect(JSON.parse(await readFile(join(options.output, "latest.json"), "utf8"))).toEqual(
      manifest,
    );
  });
});

test("rejects ambiguous updater packages before writing the manifest", async () => {
  await fixture(async (options) => {
    await writeFile(
      join(options.artifacts, "vaultforge-wallet-linux-x64", "bundle", "extra.deb"),
      "extra",
    );
    await expect(prepareReleaseAssets(options)).rejects.toThrow("Expected one deb updater");
    await expect(access(join(options.output, "latest.json"))).rejects.toThrow();
  });
});

test("rejects empty signatures before publishing an unusable manifest", async () => {
  await fixture(async (options) => {
    await writeFile(
      join(
        options.artifacts,
        "vaultforge-wallet-windows-arm64",
        "bundle",
        "VaultForge Wallet.exe.sig",
      ),
      "\n",
    );
    await expect(prepareReleaseAssets(options)).rejects.toThrow("Empty updater signature");
    await expect(access(join(options.output, "latest.json"))).rejects.toThrow();
  });
});

test("rejects missing macOS updater archives despite having a DMG", async () => {
  await fixture(async (options) => {
    await rm(
      join(
        options.artifacts,
        "vaultforge-wallet-macos-arm64",
        "bundle",
        "VaultForge Wallet.app.tar.gz",
      ),
    );
    await expect(prepareReleaseAssets(options)).rejects.toThrow("Expected one app updater");
  });
});

test("rejects missing package signatures", async () => {
  await fixture(async (options) => {
    await rm(
      join(options.artifacts, "vaultforge-wallet-linux-x64", "bundle", "VaultForge Wallet.deb.sig"),
    );
    await expect(prepareReleaseAssets(options)).rejects.toThrow();
    await expect(access(join(options.output, "latest.json"))).rejects.toThrow();
  });
});
