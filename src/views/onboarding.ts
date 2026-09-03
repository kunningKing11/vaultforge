import eyeOffIcon from "../assets/icons/eye-off.svg?raw";
import eyeIcon from "../assets/icons/eye.svg?raw";
import { fiatCurrencies } from "../currencies";
import { escapeHtml } from "../format";
import { networks } from "../networks";
import { appState } from "../state";
import { themes } from "../theme";
import { featureCard, inlineIcon, walletPasswordMeter } from "./shared";

export function splashView() {
  return `
    <section class="mx-auto flex min-h-[80vh] max-w-5xl items-center justify-center">
      <div class="glass rounded-[2rem] p-10 text-center">
        <p class="theme-text-accent text-sm font-bold uppercase tracking-[0.4em]">VaultForge</p>
        <h1 class="mt-4 text-4xl font-black">Loading wallet...</h1>
      </div>
    </section>
  `;
}

export function onboardingView() {
  const wizard = appState.onboarding;
  const stepIndicator = (n: number, label: string) =>
    `<div class="flex items-center gap-2 ${wizard.step === n ? "text-white" : "text-slate-500"}">
      <span class="flex h-7 w-7 items-center justify-center rounded-full text-xs font-bold ${wizard.step === n ? "bg-white/20" : "bg-white/5"}">${n}</span>
      <span class="text-sm font-bold">${label}</span>
    </div>`;

  const titles: Record<number, string> = {
    1: "Get started",
    2: wizard.flow === "import" ? "Import wallet" : "Create wallet",
    3: "Recovery phrase",
    4: "Appearance",
    5: "Settings",
    6: "Confirm",
  };

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
            <h2 class="text-2xl font-black">${titles[wizard.step]}</h2>
          </div>
        </div>

        <div class="mb-6 flex gap-3 overflow-x-auto">
          ${stepIndicator(1, "Flow")}
          ${stepIndicator(2, "Identity")}
          ${stepIndicator(3, "Seed")}
          ${stepIndicator(4, "Appearance")}
          ${stepIndicator(5, "Settings")}
          ${stepIndicator(6, "Backup")}
        </div>

        ${wizard.step === 1 ? step1() : ""}
        ${wizard.step === 2 ? step2() : ""}
        ${wizard.step === 3 ? step3() : ""}
        ${wizard.step === 4 ? step4() : ""}
        ${wizard.step === 5 ? step5() : ""}
        ${wizard.step === 6 ? step6() : ""}
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
  const wizard = appState.onboarding;
  const isImport = wizard.flow === "import";
  const passwordRevealLabel = wizard.walletPasswordVisible
    ? "Hide wallet passwords"
    : "Show wallet passwords";

  const passwordIcon = wizard.walletPasswordVisible ? eyeOffIcon : eyeIcon;
  return `
    <div class="space-y-4">
      ${
        !isImport
          ? `
        <label class="block space-y-2">
          <span class="text-sm font-bold text-slate-300">Wallet name</span>
          <input class="field" data-wizard-field="name" value="${escapeHtml(wizard.name)}" placeholder="Primary Vault" />
        </label>
      `
          : ""
      }
      <label class="block space-y-2">
        <span class="text-sm font-bold text-slate-300">Wallet password</span>
        <div class="relative">
          <input class="field" data-wizard-field="walletPassword" type="${wizard.walletPasswordVisible ? "text" : "password"}" minlength="8" placeholder="Minimum 8 characters" data-wallet-password-input value="${escapeHtml(wizard.walletPassword)}" />
          <button class="absolute right-3 top-1/2 -translate-y-1/2" type="button" data-action="toggle-wallet-password-visibility" aria-label="${passwordRevealLabel}" aria-pressed="${wizard.walletPasswordVisible}">
            ${inlineIcon({ svg: passwordIcon })}
          </button>
        </div>
      </label>
      ${walletPasswordMeter(wizard.walletPassword)}
      <label class="block space-y-2">
        <span class="text-sm font-bold text-slate-300">Confirm wallet password</span>
        <div class="relative">
          <input class="field" data-wizard-field="confirmWalletPassword" type="${wizard.walletPasswordVisible ? "text" : "password"}" minlength="8" value="${escapeHtml(wizard.confirmWalletPassword)}" />
          <button class="absolute right-3 top-1/2 -translate-y-1/2" type="button" data-action="toggle-wallet-password-visibility" aria-label="${passwordRevealLabel}" aria-pressed="${wizard.walletPasswordVisible}">
            ${inlineIcon({ svg: passwordIcon })}
          </button>
        </div>
      </label>
      <div class="flex gap-3 pt-2">
        <button class="btn-secondary flex-1" type="button" data-action="setup-prev">Back</button>
        <button class="btn-primary flex-1" type="button" data-action="setup-next">Next</button>
      </div>
    </div>
  `;
}

function step3() {
  const wizard = appState.onboarding;
  const isImport = wizard.flow === "import";

  if (isImport) {
    return `
      <div class="space-y-4">
        <p class="text-sm text-slate-300">Enter your recovery phrase exactly as you wrote it down.</p>
        <label class="block space-y-2">
          <span class="text-sm font-bold text-slate-300">Recovery phrase</span>
          <textarea class="field min-h-28 resize-none" data-wizard-field="mnemonic" placeholder="12, 15, 18, 21, or 24 word phrase">${escapeHtml(wizard.recoveryPhrase)}</textarea>
        </label>
        <div class="flex gap-3 pt-2">
          <button class="btn-secondary flex-1" type="button" data-action="setup-prev">Back</button>
          <button class="btn-primary flex-1" type="button" data-action="setup-next">Next</button>
        </div>
      </div>
    `;
  }

  const counts = [12, 15, 18, 21, 24] as const;
  const labels: Record<number, { title: string; desc: string }> = {
    12: { title: "12 words", desc: "Most common. Good balance of security and simplicity." },
    24: { title: "24 words", desc: "Maximum entropy. Most secure option available." },
  };
  const customCounts = [15, 18, 21];

  return `
    <div class="space-y-4">
      <p class="text-sm text-slate-300">Choose how long your recovery phrase should be. Longer phrases are harder to brute-force.</p>
      <div class="grid grid-cols-2 gap-3">
        ${counts
          .filter((c) => c === 12 || c === 24)
          .map(
            (c) => `
          <button type="button" data-action="setup-wordcount" data-wordcount="${c}"
            class="rounded-xl border px-4 py-4 text-left transition ${wizard.wordCount === c ? "border-white/40 bg-white/10" : "border-white/10 hover:bg-white/5"}">
            <p class="font-black text-lg">${labels[c].title}</p>
            <p class="mt-1 text-xs text-slate-400">${labels[c].desc}</p>
          </button>
        `,
          )
          .join("")}
      </div>
      <div class="rounded-xl border ${customCounts.includes(wizard.wordCount) ? "border-white/40 bg-white/10" : "border-white/10"} px-4 py-4">
        <p class="font-black text-lg">Custom length</p>
        <p class="mt-1 text-xs text-slate-400">Choose a non-standard word count.</p>
        <select class="field mt-3" data-wizard-field="customWordCount">
          <option value="0" ${!customCounts.includes(wizard.wordCount) ? "selected" : ""}>Select word count...</option>
          <option value="15" ${wizard.wordCount === 15 ? "selected" : ""}>15 words</option>
          <option value="18" ${wizard.wordCount === 18 ? "selected" : ""}>18 words</option>
          <option value="21" ${wizard.wordCount === 21 ? "selected" : ""}>21 words</option>
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
  const wizard = appState.onboarding;

  return `
    <div class="space-y-4">
      <p class="text-sm text-slate-300">Choose the appearance of the wallet interface.</p>
      <div class="grid grid-cols-2 gap-3 sm:grid-cols-3">
        ${Object.entries(themes)
          .map(
            ([id, theme]) => `
            <button type="button" data-action="setup-theme" data-theme="${id}"
              class="rounded-xl border px-4 py-4 text-left transition ${wizard.appearance === id ? "border-white/40 bg-white/10" : "border-white/10 hover:bg-white/5"}">
              <p class="font-black text-lg">${theme.name}</p>
            </button>
          `,
          )
          .join("")}
      </div>
      <div class="flex gap-3 pt-2">
        <button class="btn-secondary flex-1" type="button" data-action="setup-prev">Back</button>
        <button class="btn-primary flex-1" type="button" data-action="setup-next">Next</button>
      </div>
    </div>
  `;
}

function step5() {
  const wizard = appState.onboarding;
  const autoLockOptions = [
    { label: "Off", value: "0" },
    { label: "5 minutes", value: "300" },
    { label: "10 minutes", value: "600" },
    { label: "15 minutes", value: "900" },
    { label: "30 minutes", value: "1800" },
    { label: "1 hour", value: "3600" },
  ];
  const currentAutoLock =
    wizard.autoLockTimeoutSecs === null ? "0" : String(wizard.autoLockTimeoutSecs);

  return `
    <div class="space-y-5">
      <div>
        <h3 class="mb-3 text-sm font-bold uppercase tracking-wider text-slate-400">Networks</h3>
        <div class="grid grid-cols-2 gap-2 sm:grid-cols-3">
          ${networks
            .map(
              (n) => `
            <label class="flex items-center gap-2 rounded-lg border border-white/10 px-3 py-2 text-sm transition hover:bg-white/5 cursor-pointer">
              <input type="checkbox" data-wizard-network="${n.id}" ${wizard.enabledNetworks.includes(n.id) ? "checked" : ""} class="accent-white" />
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
      <div>
        <h3 class="mb-2 text-sm font-bold uppercase tracking-wider text-slate-400">Display currency</h3>
        <select class="field" data-wizard-currency>
          ${fiatCurrencies
            .map(
              ({ code, label }) =>
                `<option value="${escapeHtml(code)}" ${code === wizard.fiatCurrency ? "selected" : ""}>${escapeHtml(label)} (${escapeHtml(code)})</option>`,
            )
            .join("")}
        </select>
      </div>
      <div class="flex gap-3 pt-2">
        <button class="btn-secondary flex-1" type="button" data-action="setup-prev">Back</button>
        <button class="btn-primary flex-1" type="button" data-action="setup-next">Next</button>
      </div>
    </div>
  `;
}

function step6() {
  const wizard = appState.onboarding;
  const isImport = wizard.flow === "import";
  const recoveryPhrase = wizard.recoveryPhrase;
  const recoveryPhraseDisplay = wizard.recoveryPhraseVisible
    ? recoveryPhrase
    : recoveryPhrase.replace(/\S/g, "•");
  const revealLabel = wizard.recoveryPhraseVisible
    ? "Hide recovery phrase"
    : "Show recovery phrase";
  const recoveryPhraseIcon = wizard.recoveryPhraseVisible ? eyeOffIcon : eyeIcon;

  return `
    <form class="space-y-4" data-action="wallet-setup">
      <div class="rounded-xl border border-white/10 bg-white/5 p-4">
        <h3 class="mb-2 font-black">${isImport ? "Import summary" : "Backup your recovery phrase"}</h3>
        ${
          isImport
            ? `
          <p class="text-sm text-slate-300">Confirm the recovery phrase you provided before importing your wallet.</p>
        `
            : `
          <p class="text-sm text-slate-300">Write down your ${wizard.wordCount}-word recovery phrase and store it securely. It is the only way to recover your wallet.</p>
        `
        }
        <div class="relative mt-3">
          <textarea class="field min-h-26 resize-none pr-12 font-mono text-sm" readonly aria-label="Recovery phrase">${escapeHtml(recoveryPhraseDisplay)}</textarea>
          <button class="absolute right-3 top-3 flex h-8 w-8 items-center justify-center text-slate-300 transition hover:text-white" type="button" data-action="toggle-recovery-phrase" aria-label="${revealLabel}" aria-pressed="${wizard.recoveryPhraseVisible}">
            ${inlineIcon({ svg: recoveryPhraseIcon })}
          </button>
        </div>
        ${
          !isImport
            ? `
          <label class="mt-3 flex items-start gap-2 rounded-lg border border-white/10 bg-black/10 p-3 text-sm text-slate-300">
            <input class="mt-1 accent-white" type="checkbox" data-wizard-field="acknowledgedBackup" ${wizard.acknowledgedBackup ? "checked" : ""} />
            <span>I have written down this recovery phrase and understand it is required to recover the wallet.</span>
          </label>
        `
            : ""
        }
      </div>
      <div class="rounded-xl border border-white/10 bg-white/5 p-4 text-sm text-slate-300">
        <p><span class="font-bold text-white">Recovery phrase:</span> ${wizard.wordCount} words</p>
        <p><span class="font-bold text-white">Networks:</span> ${wizard.enabledNetworks.length} enabled</p>
        <p><span class="font-bold text-white">Auto-lock:</span> ${wizard.autoLockTimeoutSecs ? `${wizard.autoLockTimeoutSecs / 60} min` : "Off"}</p>
        <p><span class="font-bold text-white">Display currency:</span> ${escapeHtml(wizard.fiatCurrency)}</p>
      </div>
      <div class="flex gap-3 pt-2">
        <button class="btn-secondary flex-1" type="button" data-action="setup-prev">Back</button>
        <button class="btn-primary flex-1" type="submit" data-action="wallet-setup">${isImport ? "Import wallet" : "Generate wallet"}</button>
      </div>
    </form>
  `;
}
