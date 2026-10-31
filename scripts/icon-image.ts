/** Validate pinned image formats before bundling issuer artwork. */
export function validateIconImage(bytes: Uint8Array, file: string): void {
  if (file.endsWith(".svg")) {
    const text = new TextDecoder().decode(bytes);
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
      bytes.length >= 33 &&
      [137, 80, 78, 71, 13, 10, 26, 10].every((byte, index) => bytes[index] === byte) &&
      new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(8) === 13 &&
      new TextDecoder().decode(bytes.subarray(12, 16)) === "IHDR" &&
      new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(16) > 0 &&
      new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(20) > 0
    )
      return;
  } else if (/\.jpe?g$/.test(file)) {
    if (
      bytes.length >= 4 &&
      bytes[0] === 0xff &&
      bytes[1] === 0xd8 &&
      bytes[2] === 0xff &&
      bytes[bytes.length - 2] === 0xff &&
      bytes[bytes.length - 1] === 0xd9
    )
      return;
  }
  throw new Error(`Invalid or unsafe image: ${file}`);
}
