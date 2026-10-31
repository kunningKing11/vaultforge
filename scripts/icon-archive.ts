import { unzipSync } from "fflate";

export function extractIconArchiveMember(archive: Uint8Array, member: string): Uint8Array {
  let matches = 0;
  const files = unzipSync(archive, {
    filter: (file) => {
      if (file.name !== member) return false;
      if (++matches > 1 || file.originalSize > 20 * 1024 * 1024)
        throw new Error(`Invalid archive member: ${member}`);
      return true;
    },
  });
  if (matches !== 1 || !files[member]) throw new Error(`Archive member not found: ${member}`);
  return files[member];
}
