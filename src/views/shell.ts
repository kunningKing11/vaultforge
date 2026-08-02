import { escapeHtml, shortAddress } from "../format";
import { appState } from "../state";
import { walletView } from "./wallet";

export function walletShell() {
  if (!appState.session) return "";
  return `
    <div class="mx-auto grid max-w-[1500px] gap-5 pb-24 lg:grid-cols-[280px_1fr] lg:pb-0">
      <aside class="glass hidden rounded-[2rem] p-5 lg:sticky lg:top-5 lg:block lg:h-[calc(100vh-2.5rem)]">
        <div class="mb-8 flex items-center gap-3">
          <div class="theme-badge-accent flex h-12 w-12 items-center justify-center rounded-2xl text-xl font-black">VF</div>
          <div><p class="font-black">${escapeHtml(appState.session.wallet_name ?? "VaultForge")}</p></div>
        </div>
        <nav class="space-y-2">
          ${navButton("dashboard", "Dashboard")}
          ${navButton("send", "Send")}
          ${navButton("receive", "Receive")}
          ${navButton("swap", "Swap")}
          ${navButton("assets", "Assets")}
          ${navButton("activity", "Activity")}
          ${navButton("settings", "Settings")}
        </nav>
        <div class="mt-8 rounded-2xl border border-white/10 bg-white/[0.03] p-4">
          <p class="text-xs uppercase tracking-[0.25em] text-slate-500">Address</p>
          <p class="mt-2 break-all font-mono text-sm text-slate-300">${escapeHtml(shortAddress(appState.session.address))}</p>
          <button class="btn-secondary mt-4 w-full text-sm" data-action="copy-address" type="button">Copy</button>
        </div>
      </aside>
      <section class="space-y-5">
        ${topBar()}
        ${walletView()}
      </section>
      ${mobileNav()}
    </div>
  `;
}

function topBar() {
  if (!appState.session) return "";
  return `
    <header class="glass flex flex-col gap-4 rounded-[2rem] p-5 sm:flex-row sm:items-center sm:justify-between">
      <div>
        <p class="text-sm uppercase tracking-[0.3em] text-slate-500">Overview</p>
        <h1 class="mt-1 text-4xl font-black">${escapeHtml(appState.session.wallet_name ?? "𝕍𝕒𝕦𝕝𝕥𝔽𝕠𝕣𝕘𝕖")}</h1>
      </div>
      <div class="flex flex-wrap gap-3">
        <button class="btn-secondary" data-action="refresh" type="button">Refresh</button>
        <button class="btn-secondary" data-action="lock" type="button">Lock</button>
        <button class="btn-primary" data-view="send" type="button">Send funds</button>
      </div>
    </header>
  `;
}

function navButton(view: string, label: string) {
  return `<button class="nav-item ${appState.currentView === view ? "active" : ""}" data-view="${view}" type="button">${label}</button>`;
}

function mobileNav() {
  return `
    <nav class="mobile-nav glass">
      ${mobileNavButton("dashboard", "Home")}
      ${mobileNavButton("send", "Send")}
      ${mobileNavButton("receive", "Receive")}
      ${mobileNavButton("activity", "Activity")}
      ${mobileNavButton("settings", "Secure")}
    </nav>
  `;
}

function mobileNavButton(view: string, label: string) {
  return `<button class="mobile-nav-item ${appState.currentView === view ? "active" : ""}" data-view="${view}" type="button">${label}</button>`;
}
