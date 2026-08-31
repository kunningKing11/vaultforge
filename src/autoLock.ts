import type { WalletState } from "./state";

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
