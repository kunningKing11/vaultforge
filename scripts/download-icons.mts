import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rename, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import manifest from "../src/icon-manifest.json";

const root = fileURLToPath(new URL("../public/icons/", import.meta.url));
const checkOnly = process.argv.includes("--check");
const checksum = (bytes: Uint8Array) => createHash("sha256").update(bytes).digest("hex");

function transformSvg(bytes: Uint8Array, transform?: string, viewBox?: string): Uint8Array {
  let text = Buffer.from(bytes).toString("utf8");
  if (transform === "inline_xml" || transform === "injective_purple") {
    text = text.replace(/viewbox=/g, "viewBox=").replace(/\sdata-v-[a-z0-9]+(?:="[^"]*")?/g, "");
    if (transform === "injective_purple") text = text.replace(/currentColor/g, "#4D3DFF");
  } else if (transform === "optimism_token") {
    const circle = text.match(/<circle\b[^>]*>(?:<\/circle>)?/);
    const paths = [...text.matchAll(/<path\b[^>]*>(?:[\s\S]*?<\/path>)/g)].slice(1);
    if (!circle || paths.length !== 1) throw new Error("Official OP token artwork missing");
    text = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="841 529 817 817">${circle[0]}${paths[0][0]}</svg>\n`;
  } else if (transform === "bitcoin_symbol" || transform === "bnb_symbol") {
    // Keep the original symbol paths; omit the wordmark and its coordinate transforms.
    const count = transform === "bitcoin_symbol" ? 2 : 1;
    const viewBox = transform === "bitcoin_symbol" ? "194 43 64 64" : "-1.6273 0 24 24";
    const paths = [...text.matchAll(/<path\b[^>]*(?:\/>|>[\s\S]*?<\/path>)/g)].slice(0, count);
    if (paths.length !== count) throw new Error("Official symbol paths missing");
    text = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${viewBox}">${paths.map((path) => path[0]).join("")}</svg>\n`;
  }
  if (viewBox !== undefined) {
    const bounds = viewBox.trim().split(/\s+/).map(Number);
    if (bounds.length !== 4 || !bounds.every(Number.isFinite) || bounds[2] <= 0 || bounds[3] <= 0)
      throw new Error("Invalid icon viewBox crop");
    // Isolate a published symbol from its wordmark without changing any paths.
    text = text.replace(/<svg\b[^>]*>/, (root) =>
      root
        .replace(/\s(?:viewBox|width|height|overflow)=(["']).*?\1/g, "")
        .replace(/>$/, ` viewBox="${bounds.join(" ")}" overflow="hidden">`),
    );
  }
  return Buffer.from(text);
}

function validateImage(bytes: Uint8Array, file: string) {
  const buffer = Buffer.from(bytes);
  if (file.endsWith(".svg")) {
    const text = buffer.toString("utf8");
    // Reject active content rather than modifying official artwork. Only the reviewed,
    // checksum-pinned files are accepted, and SVGs are served as image resources.
    if (
      /<svg\b/.test(text) &&
      !/<(?:script|foreignObject|iframe|image|animate|set)\b|<!ENTITY|<!DOCTYPE|\bon\w+\s*=|(?:href|xlink:href)\s*=|javascript:|@import|url\(\s*["']?(?!#)/i.test(
        text,
      )
    )
      return;
  }
  throw new Error(`Invalid or unsafe image: ${file}`);
}

for (const [id, asset] of Object.entries(manifest.assets)) {
  const destination = resolve(root, asset.file);
  if (
    !asset.file.endsWith(".svg") ||
    !destination.startsWith(root) ||
    !/^https:\/\//.test(asset.url)
  )
    throw new Error(`Invalid manifest entry: ${id}`);
  let bytes: Uint8Array;
  if (checkOnly) {
    bytes = await readFile(destination);
  } else {
    const response = await fetch(asset.url, { signal: AbortSignal.timeout(30000) });
    if (!response.ok) throw new Error(`${id}: HTTP ${response.status}`);
    const downloaded = new Uint8Array(await response.arrayBuffer());
    if (downloaded.length > 20 * 1024 * 1024) throw new Error(`${id}: download too large`);
    if ("archiveMember" in asset) {
      // Extract one named file to stdout; never unpack remote paths into the project.
      const temporary = await mkdtemp(resolve(tmpdir(), "vaultforge-icons-"));
      try {
        const archive = resolve(temporary, "download.zip");
        await writeFile(archive, downloaded);
        const child = Bun.spawn(["unzip", "-p", archive, asset.archiveMember], {
          stdout: "pipe",
          stderr: "pipe",
        });
        bytes = new Uint8Array(await new Response(child.stdout).arrayBuffer());
        if ((await child.exited) !== 0)
          throw new Error(`${id}: archive extraction failed (requires unzip)`);
      } finally {
        await rm(temporary, { recursive: true, force: true });
      }
    } else if ("inlineSvgLabel" in asset || "inlineSvgMatch" in asset) {
      const label =
        "inlineSvgMatch" in asset ? asset.inlineSvgMatch : `aria-label="${asset.inlineSvgLabel}"`;
      const matches = [
        ...Buffer.from(downloaded)
          .toString("utf8")
          .matchAll(/<svg\b[^>]*>[\s\S]*?<\/svg>/g),
      ]
        .map((match) => match[0])
        .filter((svg) => svg.includes(label));
      const pinned = matches.find(
        (svg) =>
          checksum(
            transformSvg(
              Buffer.from(svg),
              "svgTransform" in asset ? asset.svgTransform : undefined,
              "svgViewBox" in asset ? asset.svgViewBox : undefined,
            ),
          ) === asset.sha256,
      );
      if (!pinned) throw new Error(`${id}: reviewed official inline SVG not found`);
      bytes = Buffer.from(pinned);
    } else {
      bytes = downloaded;
    }
  }
  if (!checkOnly && ("svgTransform" in asset || "svgViewBox" in asset))
    bytes = transformSvg(
      bytes,
      "svgTransform" in asset ? asset.svgTransform : undefined,
      "svgViewBox" in asset ? asset.svgViewBox : undefined,
    );
  if (checksum(bytes) !== asset.sha256)
    throw new Error(
      `${id}: checksum changed; review the official asset before updating the manifest`,
    );
  validateImage(bytes, asset.file);
  if (!checkOnly) {
    await mkdir(dirname(destination), { recursive: true });
    await writeFile(`${destination}.tmp`, bytes);
    await rename(`${destination}.tmp`, destination);
  }
  console.log(`${checkOnly ? "Verified" : "Downloaded"} ${id}`);
}
