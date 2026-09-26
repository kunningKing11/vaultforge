const DISCLAIMER_STORAGE_KEY = "vaultforge-disclaimer-v1";

export function hasAcceptedDisclaimer(storage: Pick<Storage, "getItem">): boolean {
  try {
    return storage.getItem(DISCLAIMER_STORAGE_KEY) === "accepted";
  } catch {
    return false;
  }
}

export function acceptDisclaimer(storage: Pick<Storage, "setItem">): boolean {
  try {
    storage.setItem(DISCLAIMER_STORAGE_KEY, "accepted");
    return true;
  } catch {
    return false;
  }
}
