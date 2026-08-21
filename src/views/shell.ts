import lockIconUrl from "../assets/icons/lock.svg";
import { escapeHtml, shortAddress } from "../format";
import appLogoUrl from "../../src-tauri/icons/icon.svg";
import { appState } from "../state";
import { walletView } from "./wallet";

export function walletShell() {
  if (!appState.session) return "";
  return `
    <div class="mx-auto grid max-w-[1500px] gap-5 pb-24 lg:grid-cols-[280px_1fr] lg:pb-0">
      <aside class="glass hidden rounded-[2rem] p-5 lg:sticky lg:top-5 lg:h-[calc(100vh-2.5rem)] lg:flex lg:min-h-0 lg:flex-col">
        <div class="mb-8 flex shrink-0 items-center gap-3">
          <img class="h-12 w-12 rounded-2xl" src="${appLogoUrl}" alt="VaultForge App Logo" />
          <div><p class="font-black">${escapeHtml(appState.session.wallet_name ?? "")}</p></div>
        </div>
        <div class="sidebar-nav-shell min-h-0 flex-1">
          <nav id="sidebar-nav" class="sidebar-nav min-h-0 flex-1 space-y-2" data-sidebar-scroll tabindex="0" aria-label="Wallet sections">
            ${navButton("dashboard", "Dashboard")}
            ${navButton("send", "Send")}
            ${navButton("receive", "Receive")}
            ${navButton("swap", "Swap")}
            ${navButton("assets", "Assets")}
            ${navButton("activity", "Activity")}
            ${navButton("settings", "Settings")}
          </nav>
          <div class="sidebar-scrollbar" data-sidebar-scrollbar role="scrollbar" tabindex="0" aria-controls="sidebar-nav" aria-label="Scroll wallet sections vertically" aria-orientation="vertical" aria-valuemin="0" aria-valuemax="0" aria-valuenow="0">
            <div class="sidebar-scrollbar-thumb" data-sidebar-scrollbar-thumb></div>
          </div>
        </div>
        <div class="mt-8 shrink-0 rounded-2xl border border-white/10 bg-white/[0.03] p-4">
          <p class="text-xs uppercase tracking-[0.25em] text-slate-500">Address</p>
          <p class="mt-2 break-all font-mono text-sm font-bold text-slate-300">${escapeHtml(shortAddress(appState.session.address))}</p>
          <button class="btn-secondary mt-4 w-full text-sm font-bold" data-action="copy-address" type="button">Copy</button>
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
        <h1 class="mt-1 text-4xl font-black">${escapeHtml(appState.session.wallet_name ?? "")}</h1>
      </div>
      <div class="flex flex-wrap gap-3">
        <button class="btn-secondary" data-action="refresh" type="button">Refresh</button>
        <button class="btn-secondary inline-flex items-center gap-2" data-action="lock" type="button"><img class="h-4 w-4" src="${lockIconUrl}" alt="" />Lock</button>
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
