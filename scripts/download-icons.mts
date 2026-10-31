import { mkdir, rename } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import manifest from "../src/icon-manifest.json";
import { extractIconArchiveMember } from "./icon-archive";
import { validateIconImage } from "./icon-image";

const root = fileURLToPath(new URL("../public/icons/", import.meta.url));
const checkOnly = Bun.argv.includes("--check");
const checksum = (bytes: Uint8Array) => Bun.CryptoHasher.hash("sha256", bytes, "hex");

function transformSvg(bytes: Uint8Array, transform?: string, viewBox?: string): Uint8Array {
  let text = new TextDecoder().decode(bytes);
  if (transform === "inline_xml" || transform === "injective_purple") {
    text = text.replace(/viewbox=/g, "viewBox=").replace(/\sdata-v-[a-z0-9]+(?:="[^"]*")?/g, "");
    if (transform === "injective_purple") text = text.replace(/currentColor/g, "#4D3DFF");
  } else if (transform === "optimism_token") {
    const circle = text.match(/<circle\b[^>]*>(?:<\/circle>)?/);
    const paths = [...text.matchAll(/<path\b[^>]*>(?:[\s\S]*?<\/path>)/g)].slice(1);
    if (!circle || paths.length !== 1) throw new Error("Official OP token artwork missing");
    text = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="841 529 817 817">${circle[0]}${paths[0][0]}</svg>\n`;
  } else if (transform === "render_token") {
    // Preserve the issuer's symbol paths with the requested white-on-red treatment.
    const paths = [...text.matchAll(/<path\b[^>]*(?:\/>|>[\s\S]*?<\/path>)/g)].slice(0, 2);
    if (paths.length !== 2) throw new Error("Official Render symbol paths missing");
    text = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"><circle cx="24" cy="24" r="24" fill="#DF0001"/><g fill="#FFFFFF" transform="translate(4 4)">${paths.map((path) => path[0].replace(/\sclass="[^"]*"/g, "")).join("")}</g></svg>\n`;
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
  return new TextEncoder().encode(text);
}

for (const [id, asset] of Object.entries(manifest.assets)) {
  const destination = resolve(root, asset.file);
  if (
    !/\.(?:svg|png|jpe?g)$/.test(asset.file) ||
    !destination.startsWith(root) ||
    !/^https:\/\//.test(asset.url)
  )
    throw new Error(`Invalid manifest entry: ${id}`);
  let bytes: Uint8Array;
  if (checkOnly) {
    bytes = await Bun.file(destination).bytes();
  } else {
    const requestHeaders = "requestHeaders" in asset ? asset.requestHeaders : undefined;
    const response = await fetch(asset.url, {
      headers: requestHeaders,
      signal: AbortSignal.timeout(30000),
    });
    if (!response.ok) throw new Error(`${id}: HTTP ${response.status}`);
    const downloaded = new Uint8Array(await response.arrayBuffer());
    if (downloaded.length > 20 * 1024 * 1024) throw new Error(`${id}: download too large`);
    if ("archiveMember" in asset) {
      bytes = extractIconArchiveMember(downloaded, asset.archiveMember);
    } else if ("inlineSvgLabel" in asset || "inlineSvgMatch" in asset) {
      const label =
        "inlineSvgMatch" in asset ? asset.inlineSvgMatch : `aria-label="${asset.inlineSvgLabel}"`;
      const matches = [
        ...new TextDecoder().decode(downloaded).matchAll(/<svg\b[^>]*>[\s\S]*?<\/svg>/g),
      ]
        .map((match) => match[0])
        .filter((svg) => svg.includes(label));
      const pinned = matches.find(
        (svg) =>
          checksum(
            transformSvg(
              new TextEncoder().encode(svg),
              "svgTransform" in asset ? asset.svgTransform : undefined,
              "svgViewBox" in asset ? asset.svgViewBox : undefined,
            ),
          ) === asset.sha256,
      );
      if (!pinned) throw new Error(`${id}: reviewed official inline SVG not found`);
      bytes = new TextEncoder().encode(pinned);
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
  validateIconImage(bytes, asset.file);
  if (!checkOnly) {
    await mkdir(dirname(destination), { recursive: true });
    await Bun.write(`${destination}.tmp`, bytes);
    await rename(`${destination}.tmp`, destination);
  }
  console.log(`${checkOnly ? "Verified" : "Downloaded"} ${id}`);
}
