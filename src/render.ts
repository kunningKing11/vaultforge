import { appRoot } from "./main";
import { ensureReceiveQr } from "./qr";
import { syncHorizontalScrollbars, syncVerticalScrollbar } from "./scrollbars";
import { appState } from "./state";
import { deleteWalletModal, lockedWalletView } from "./views/locked";
import { onboardingView, splashView } from "./views/onboarding";
import { loadingBar, updateRecipientPlaceholder } from "./views/shared";
import { walletShell } from "./views/shell";

export function render() {
  appRoot.innerHTML = `
    <main class="noise min-h-screen px-4 py-5 text-slate-100 sm:px-6 lg:px-8">
      ${appState.operation.busy ? loadingBar() : ""}
      ${renderBody()}
      ${deleteWalletModal()}
    </main>
    <div class="app-scrollbar" data-vertical-scrollbar="page" role="scrollbar" tabindex="0" aria-label="Scroll page vertically" aria-orientation="vertical" aria-valuemin="0" aria-valuemax="0" aria-valuenow="0">
      <div class="app-scrollbar-thumb" data-vertical-scrollbar-thumb></div>
    </div>
  `;
  syncHorizontalScrollbars(appRoot);
  syncVerticalScrollbar();
  void ensureReceiveQr().then((needsRender) => {
    if (needsRender) render();
  });
}

function renderBody() {
  if (appState.operation.busy && appState.wallet.status === "missing") return splashView();
  if (appState.wallet.status === "missing") return onboardingView();
  if (appState.wallet.status === "locked") return lockedWalletView();
  return walletShell();
}

export { updateRecipientPlaceholder };
