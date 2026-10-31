import { expect, test } from "bun:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { buildFrontend } from "../scripts/build-frontend";

test("production build includes its frontend assets and offline icons", async () => {
  const output = await mkdtemp(join(tmpdir(), "vaultforge-build-"));
  try {
    await buildFrontend(output);
    const html = await Bun.file(join(output, "index.html")).text();
    const script = html.match(/src="\.\/([^" ]+\.js)"/)?.[1];
    const style = html.match(/href="\.\/([^" ]+\.css)"/)?.[1];
    expect(script).toBeDefined();
    expect(style).toBeDefined();
    expect(await Bun.file(join(output, script!)).exists()).toBe(true);
    expect(await Bun.file(join(output, style!)).exists()).toBe(true);
    expect(await Bun.file(join(output, script!)).text()).toContain("Minified React error");
    expect(await Bun.file(join(output, "icons/coins/usdc.svg")).bytes()).toEqual(
      await Bun.file("public/icons/coins/usdc.svg").bytes(),
    );
    const css = await Bun.file(join(output, style!)).text();
    const config = await Bun.file("src-tauri/tauri.conf.json").json();
    if (css.includes("data:font/"))
      expect(config.app.security.csp).toContain("font-src 'self' data:");
  } finally {
    await rm(output, { recursive: true, force: true });
  }
});
