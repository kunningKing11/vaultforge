import activityIcon from "../assets/icons/activity.svg?raw";
import appLogoUrl from "../../src-tauri/icons/icon.svg";
import assetsIcon from "../assets/icons/assets.svg?raw";
import copyIcon from "../assets/icons/copy.svg?raw";
import dashboardIcon from "../assets/icons/dashboard.svg?raw";
import lockIcon from "../assets/icons/lock.svg?raw";
import receiveIcon from "../assets/icons/receive.svg?raw";
import refreshIcon from "../assets/icons/refresh.svg?raw";
import sendUpIcon from "../assets/icons/send-up.svg?raw";
import sendIcon from "../assets/icons/send.svg?raw";
import settingsIcon from "../assets/icons/settings.svg?raw";
import swapIcon from "../assets/icons/swap.svg?raw";
import { escapeHtml, shortAddress } from "../format";
import { networkById } from "../networks";
import { addressKeyForNetwork, appState } from "../state";
import { inlineIcon } from "./shared";
import { walletView } from "./wallet";

export function walletShell() {
  if (!appState.session) return "";
  const displayedAddressKeys = new Set<string>();
  let counter = 0;
  let addressCards = "";
  for (const networkId of appState.session.enabled_networks) {
    const network = networkById(networkId);
    if (!network) continue;

    counter++;

    const addressKey = addressKeyForNetwork(network);
    const address = appState.session.addresses?.[addressKey];
    if (!address || displayedAddressKeys.has(addressKey)) continue;

    displayedAddressKeys.add(addressKey);
    const label = addressKey === "evm" ? "EVM" : network.name;
    addressCards += `<div class="mt-2 flex items-center gap-2"><p class="min-w-0 flex-1 break-all font-mono text-sm font-bold text-slate-300">${escapeHtml(label)}: ${escapeHtml(shortAddress(address))}</p><button class="btn-secondary copy-address-button shrink-0" data-action="copy-address" data-copy-btn-id="${counter}" type="button" aria-label="Copy ${escapeHtml(label)} address">${inlineIcon({ svg: copyIcon, sizeClass: "h-4 w-4" })}</button></div>`;
  }
  return `
    <div class="mx-auto grid max-w-[1500px] gap-5 pb-32 lg:grid-cols-[350px_1fr] lg:pb-0">
      <aside class="glass hidden rounded-[2rem] p-5 lg:sticky lg:top-5 lg:h-[calc(100vh-2.5rem)] lg:flex lg:min-h-0 lg:flex-col">
        <div class="mb-8 flex shrink-0 items-center gap-3">
          <img class="h-12 w-12 rounded-2xl" src="${appLogoUrl}" alt="VaultForge App Logo" />
          <div><p class="font-black">${escapeHtml(appState.session.wallet_name ?? "")}</p></div>
        </div>
        <div class="sidebar-nav-shell min-h-0 flex-1">
          <nav id="sidebar-nav" class="sidebar-nav min-h-0 flex-1 space-y-2" data-sidebar-scroll tabindex="0" aria-label="Wallet sections">
            ${navigationItems.map(navButton).join("")}
          </nav>
          <div class="sidebar-scrollbar" data-sidebar-scrollbar role="scrollbar" tabindex="0" aria-controls="sidebar-nav" aria-label="Scroll wallet sections vertically" aria-orientation="vertical" aria-valuemin="0" aria-valuemax="0" aria-valuenow="0">
            <div class="sidebar-scrollbar-thumb" data-sidebar-scrollbar-thumb></div>
          </div>
        </div>
        <div class="mt-8 shrink-0 rounded-2xl border border-white/10 bg-white/[0.03] p-4">
          <p class="text-xs uppercase tracking-[0.25em] text-slate-500">Addresses</p>
          ${addressCards}
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
        <button class="btn-secondary inline-flex items-center gap-2" data-action="refresh" type="button">
          ${inlineIcon({ svg: refreshIcon })}
          Refresh
        </button>
        <button class="btn-secondary inline-flex items-center gap-2" data-action="lock" type="button">
          ${inlineIcon({ svg: lockIcon })}
          Lock
        </button>
        <button class="btn-primary inline-flex items-center gap-2" data-view="send" type="button">
          ${inlineIcon({ svg: sendIcon })}
          Send funds
        </button>
      </div>
    </header>
  `;
}

function navButton({ view, label, icon }: NavigationItem) {
  return `<button class="nav-item inline-flex items-center gap-2 ${appState.currentView === view ? "active" : ""}" data-view="${view}" type="button">${inlineIcon({ svg: icon })}${label}</button>`;
}

function mobileNav() {
  return `
    <nav class="mobile-nav glass rounded-[2rem]" aria-label="Wallet sections">
      ${navigationItems.map(mobileNavButton).join("")}
    </nav>
  `;
}

function mobileNavButton({ view, label, icon }: NavigationItem) {
  return `<button class="mobile-nav-item ${appState.currentView === view ? "active" : ""}" data-view="${view}" type="button" aria-label="${label}">${inlineIcon({ svg: icon, sizeClass: "mobile-nav-icon" })}<span>${label}</span></button>`;
}

interface NavigationItem {
  view: string;
  label: string;
  icon: string;
}

const navigationItems: NavigationItem[] = [
  {
    view: "dashboard",
    label: "Dashboard",
    icon: dashboardIcon,
  },
  {
    view: "send",
    label: "Send",
    icon: sendUpIcon,
  },
  {
    view: "receive",
    label: "Receive",
    icon: receiveIcon,
  },
  {
    view: "swap",
    label: "Swap",
    icon: swapIcon,
  },
  {
    view: "assets",
    label: "Assets",
    icon: assetsIcon,
  },
  {
    view: "activity",
    label: "Activity",
    icon: activityIcon,
  },
  {
    view: "settings",
    label: "Settings",
    icon: settingsIcon,
  },
];
