import type { WalletState } from "./state";

export const autoLockOptions = [
  { label: "Off", value: "0" },
  { label: "5 minutes", value: "300" },
  { label: "10 minutes", value: "600" },
  { label: "15 minutes", value: "900" },
  { label: "30 minutes", value: "1800" },
  { label: "1 hour", value: "3600" },
];

let timer: number | null = null;
let lastActivity = Date.now();

export function recordWalletActivity(): void {
  lastActivity = Date.now();
}

export function stopAutoLock(): void {
  if (timer !== null) {
    window.clearTimeout(timer);
    timer = null;
  }
}

export function syncAutoLock(wallet: WalletState, onTimeout: () => void): void {
  stopAutoLock();
  if (wallet.status !== "unlocked" || !wallet.autoLockTimeoutSecs) return;

  const timeoutMs = wallet.autoLockTimeoutSecs * 1000;
  const check = () => {
    const remaining = timeoutMs - (Date.now() - lastActivity);
    if (remaining <= 0) {
      onTimeout();
      return;
    }
    timer = window.setTimeout(check, remaining);
  };
  check();
}
