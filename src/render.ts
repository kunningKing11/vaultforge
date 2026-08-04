import { appRoot } from "./main";
import { ensureReceiveQr } from "./qr";
import { appState } from "./state";
import { lockedDeleteWalletModal, lockedWalletView } from "./views/locked";
import { onboardingView, splashView } from "./views/onboarding";
import { loadingBar, updateRecipientPlaceholder } from "./views/shared";
import { walletShell } from "./views/shell";
import { syncHorizontalScrollbars, syncVerticalScrollbar } from "./scrollbars";

export function render() {
  appRoot.innerHTML = `
    <main class="noise min-h-screen px-4 py-5 text-slate-100 sm:px-6 lg:px-8">
      ${appState.busy ? loadingBar() : ""}
      ${renderBody()}
      ${lockedDeleteWalletModal()}
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
  if (!appState.session && appState.busy) return splashView();
  if (!appState.session?.has_wallet) return onboardingView();
  if (appState.session?.locked) return lockedWalletView();
  return walletShell();
}

export { updateRecipientPlaceholder };
