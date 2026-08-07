import { networks } from "../networks";
import { appState } from "../state";
import { featureCard, passphraseMeter } from "./shared";

export function splashView() {
  return `
    <section class="mx-auto flex min-h-[80vh] max-w-5xl items-center justify-center">
      <div class="glass rounded-[2rem] p-10 text-center">
        <p class="theme-text-accent text-sm font-bold uppercase tracking-[0.4em]">VaultForge</p>
        <h1 class="mt-4 text-4xl font-black">Loading wallet core</h1>
      </div>
    </section>
  `;
}

export function onboardingView() {
  const w = appState.setupWizard;
  const stepIndicator = (n: number, label: string) =>
    `<div class="flex items-center gap-2 ${w.step === n ? "text-white" : "text-slate-500"}">
      <span class="flex h-7 w-7 items-center justify-center rounded-full text-xs font-bold ${w.step === n ? "bg-white/20" : "bg-white/5"}">${n}</span>
      <span class="text-sm font-bold">${label}</span>
    </div>`;

  return `
    <section class="mx-auto grid min-h-[88vh] max-w-7xl items-center gap-8 lg:grid-cols-[1fr_0.95fr]">
      <div class="space-y-8">
        <div>
          <h1 class="max-w-3xl text-5xl font-black tracking-tight text-white sm:text-7xl">Control crypto from a local-first command center.</h1>
          <p class="mt-6 max-w-2xl text-lg leading-8 text-slate-300">The last wallet you'll ever need.</p>
          <p class="mt-6 max-w-2xl text-lg leading-8 text-slate-300">Supports 10 major chains, with more coming soon.</p>
          <p class="mt-6 max-w-2xl text-lg leading-8 text-slate-300">Support for Bitcoin, EVM chains, Solana, and Tron is already here, with support for Ripple and Zcash next.</p>
        </div>
        <div class="grid gap-4 sm:grid-cols-3">
          ${featureCard("Blazing fast Rust backend", "For maximum performance and security.")}
          ${featureCard("Fast UI", "Vite, TypeScript, and TailwindCSS power the frontend.")}
          ${featureCard("Multi-chain support", "Support for 10 major chains, with more coming soon.")}
        </div>
      </div>
      <div class="glass rounded-[2rem] p-6 sm:p-8">
        <div class="mb-6 flex items-center justify-between">
          <div>
            <p class="text-sm font-bold uppercase tracking-[0.3em] text-slate-500">Wallet Setup</p>
            <h2 class="text-2xl font-black">${w.step === 1 ? "Get started" : w.step === 2 ? (w.flow === "import" ? "Import wallet" : "Create wallet") : w.step === 3 ? "Settings" : "Confirm"}</h2>
          </div>
        </div>

        <div class="mb-6 flex gap-4">
          ${stepIndicator(1, "Flow")}
          ${stepIndicator(2, "Identity")}
          ${stepIndicator(3, "Settings")}
          ${stepIndicator(4, "Backup")}
        </div>

        ${w.step === 1 ? step1() : ""}
        ${w.step === 2 ? step2() : ""}
        ${w.step === 3 ? step3() : ""}
        ${w.step === 4 ? step4() : ""}
      </div>
    </section>
  `;
}

function step1() {
  return `
    <div class="space-y-4">
      <p class="text-slate-300">Create a new wallet or import an existing one.</p>
      <button class="btn-primary w-full" type="button" data-action="setup-create">Create new wallet</button>
      <button class="btn-secondary w-full" type="button" data-action="setup-import">Import existing wallet</button>
    </div>
  `;
}

function step2() {
  const w = appState.setupWizard;
  const isImport = w.flow === "import";
  return `
    <div class="space-y-4">
      ${
        !isImport
          ? `
        <label class="block space-y-2">
          <span class="text-sm font-bold text-slate-300">Wallet name</span>
          <input class="field" data-wizard-field="name" value="${w.name}" placeholder="Primary Vault" />
        </label>
      `
          : ""
      }
      <label class="block space-y-2">
        <span class="text-sm font-bold text-slate-300">Passphrase</span>
        <input class="field" data-wizard-field="passphrase" type="password" minlength="8" placeholder="Minimum 8 characters" data-passphrase-input value="${w.passphrase}" />
      </label>
      ${passphraseMeter()}
      <label class="block space-y-2">
        <span class="text-sm font-bold text-slate-300">Confirm passphrase</span>
        <input class="field" data-wizard-field="confirmPassphrase" type="password" minlength="8" value="${w.confirmPassphrase}" />
      </label>
      ${
        isImport
          ? `
        <label class="block space-y-2">
          <span class="text-sm font-bold text-slate-300">Recovery phrase</span>
          <textarea class="field min-h-28" data-wizard-field="mnemonic" placeholder="12 or 24 word phrase">${w.mnemonic}</textarea>
        </label>
      `
          : ""
      }
      <div class="flex gap-3 pt-2">
        <button class="btn-secondary flex-1" type="button" data-action="setup-prev">Back</button>
        <button class="btn-primary flex-1" type="button" data-action="setup-next">Next</button>
      </div>
    </div>
  `;
}

function step3() {
  const w = appState.setupWizard;
  const autoLockOptions = [
    { label: "Off", value: "0" },
    { label: "5 minutes", value: "300" },
    { label: "15 minutes", value: "900" },
    { label: "30 minutes", value: "1800" },
    { label: "1 hour", value: "3600" },
  ];
  const currentAutoLock = w.autoLockTimeoutSecs === null ? "0" : String(w.autoLockTimeoutSecs);

  return `
    <div class="space-y-5">
      <div>
        <h3 class="mb-3 text-sm font-bold uppercase tracking-wider text-slate-400">Networks</h3>
        <div class="grid grid-cols-2 gap-2 sm:grid-cols-3">
          ${networks
            .map(
              (n) => `
            <label class="flex items-center gap-2 rounded-lg border border-white/10 px-3 py-2 text-sm transition hover:bg-white/5 cursor-pointer">
              <input type="checkbox" data-wizard-network="${n.id}" ${w.enabledNetworks.includes(n.id) ? "checked" : ""} class="accent-white" />
              <span class="font-bold">${n.nickname ?? n.name}</span>
            </label>
          `,
            )
            .join("")}
        </div>
      </div>
      <div>
        <h3 class="mb-2 text-sm font-bold uppercase tracking-wider text-slate-400">Auto-lock timeout</h3>
        <select class="field" data-wizard-autolock>
          ${autoLockOptions.map((o) => `<option value="${o.value}" ${o.value === currentAutoLock ? "selected" : ""}>${o.label}</option>`).join("")}
        </select>
      </div>
      <div class="flex gap-3 pt-2">
        <button class="btn-secondary flex-1" type="button" data-action="setup-prev">Back</button>
        <button class="btn-primary flex-1" type="button" data-action="setup-next">Next</button>
      </div>
    </div>
  `;
}

function step4() {
  const w = appState.setupWizard;
  const isImport = w.flow === "import";

  return `
    <div class="space-y-4">
      <div class="rounded-xl border border-white/10 bg-white/5 p-4">
        <h3 class="mb-2 font-black">${isImport ? "Import summary" : "Backup your recovery phrase"}</h3>
        ${
          isImport
            ? `
          <p class="text-sm text-slate-300">Your wallet will be imported with the recovery phrase you provided. Make sure it is correct.</p>
        `
            : `
          <p class="text-sm text-slate-300">Write down your 12-word recovery phrase and store it securely. It is the only way to recover your wallet.</p>
          <p class="mt-2 rounded-lg bg-white/5 p-3 font-mono text-sm text-white break-words">${w.mnemonic || "Generated on confirm — phrase will appear here"}</p>
        `
        }
      </div>
      <div class="rounded-xl border border-white/10 bg-white/5 p-4 text-sm text-slate-300">
        <p><span class="font-bold text-white">Networks:</span> ${w.enabledNetworks.length} enabled</p>
        <p><span class="font-bold text-white">Auto-lock:</span> ${w.autoLockTimeoutSecs ? `${w.autoLockTimeoutSecs / 60} min` : "Off"}</p>
      </div>
      <div class="flex gap-3 pt-2">
        <button class="btn-secondary flex-1" type="button" data-action="setup-prev">Back</button>
        <button class="btn-primary flex-1" type="submit" data-action="wallet-setup">${isImport ? "Import wallet" : "Generate wallet"}</button>
      </div>
    </div>
  `;
}
