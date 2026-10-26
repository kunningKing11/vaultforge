import { expect, test } from "bun:test";

import { validateIconImage } from "../scripts/icon-image";

test("accepts the pinned official SVG, PNG and JPEG formats", async () => {
  for (const file of ["usds.svg", "fartcoin.png", "melania.jpg"]) {
    const bytes = new Uint8Array(
      await Bun.file(new URL(`../public/icons/coins/${file}`, import.meta.url)).arrayBuffer(),
    );
    expect(() => validateIconImage(bytes, file)).not.toThrow();
  }
});

test("rejects active and externally referenced SVG content", () => {
  for (const content of [
    "<script>alert(1)</script>",
    '<image href="https://example.com/icon.png"/>',
    '<path onclick="alert(1)"/>',
    '<path fill="url(https://example.com/paint)"/>',
  ]) {
    expect(() => validateIconImage(Buffer.from(`<svg>${content}</svg>`), "icon.svg")).toThrow();
  }
});

test("rejects disguised HTML responses, truncated rasters and unsupported formats", () => {
  for (const file of ["icon.svg", "icon.png", "icon.jpg", "icon.webp"]) {
    expect(() => validateIconImage(Buffer.from("<html>not an image</html>"), file)).toThrow();
    expect(() => validateIconImage(new Uint8Array(), file)).toThrow();
  }
  expect(() =>
    validateIconImage(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), "icon.png"),
  ).toThrow();
  expect(() => validateIconImage(Buffer.from([255, 216, 255]), "icon.jpg")).toThrow();
});

test("rejects image content that does not match its file extension", async () => {
  for (const [source, destination] of [
    ["usds.svg", "icon.png"],
    ["fartcoin.png", "icon.jpg"],
    ["melania.jpg", "icon.svg"],
  ]) {
    const bytes = new Uint8Array(
      await Bun.file(new URL(`../public/icons/coins/${source}`, import.meta.url)).arrayBuffer(),
    );
    expect(() => validateIconImage(bytes, destination)).toThrow();
  }
});
