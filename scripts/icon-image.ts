/** Validate pinned image formats before bundling issuer artwork. */
export function validateIconImage(bytes: Uint8Array, file: string): void {
  const buffer = Buffer.from(bytes);
  if (file.endsWith(".svg")) {
    const text = buffer.toString("utf8");
    // SVGs remain image-only: no active content, external resources or embedded rasters.
    if (
      /<svg\b/.test(text) &&
      !/<(?:script|foreignObject|iframe|image|animate|set)\b|<!ENTITY|<!DOCTYPE|\bon\w+\s*=|(?:href|xlink:href)\s*=|javascript:|@import|url\(\s*["']?(?!#)/i.test(
        text,
      )
    )
      return;
  } else if (file.endsWith(".png")) {
    if (
      buffer.length >= 33 &&
      buffer.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10])) &&
      buffer.readUInt32BE(8) === 13 &&
      buffer.toString("ascii", 12, 16) === "IHDR" &&
      buffer.readUInt32BE(16) > 0 &&
      buffer.readUInt32BE(20) > 0
    )
      return;
  } else if (/\.jpe?g$/.test(file)) {
    if (
      buffer.length >= 4 &&
      buffer[0] === 0xff &&
      buffer[1] === 0xd8 &&
      buffer[2] === 0xff &&
      buffer[buffer.length - 2] === 0xff &&
      buffer[buffer.length - 1] === 0xd9
    )
      return;
  }
  throw new Error(`Invalid or unsafe image: ${file}`);
}
