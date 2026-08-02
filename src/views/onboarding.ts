import { featureCard, passphraseMeter } from "./shared";

export function splashView() {
  return `
    <section class="mx-auto flex min-h-[80vh] max-w-5xl items-center justify-center">
      <div class="glass rounded-[2rem] p-10 text-center">
        <p class="theme-text-accent text-sm uppercase tracking-[0.4em]">VaultForge</p>
        <h1 class="mt-4 text-4xl font-black">Loading wallet core</h1>
      </div>
    </section>
  `;
}

export function onboardingView() {
  return `
    <section class="mx-auto grid min-h-[88vh] max-w-7xl items-center gap-8 lg:grid-cols-[1fr_0.95fr]">
      <div class="space-y-8">
        <div class="theme-pill-accent inline-flex rounded-full border px-4 py-2 text-sm font-bold">Desktop self-custody wallet</div>
        <div>
          <h1 class="max-w-3xl text-5xl font-black tracking-tight text-white sm:text-7xl">Control crypto from a local-first command center.</h1>
          <p class="mt-6 max-w-2xl text-lg leading-8 text-slate-300">VaultForge combines a TypeScript interface, TailwindCSS system, and Rust-powered Tauri backend for portfolio management, transfers, swaps, activity tracking, and wallet locking.</p>
        </div>
        <div class="grid gap-4 sm:grid-cols-3">
          ${featureCard("Blazing fast Rust core", "Wallet state and validations run behind Tauri commands.")}
          ${featureCard("Fast UI", "Vite, TypeScript, and Tailwind power the frontend.")}
          ${featureCard("Local-first & maximum security", "Runs fully locally on your machine.")}
        </div>
      </div>
      <div class="glass rounded-[2rem] p-6 sm:p-8">
        <div class="mb-6 flex items-center justify-between">
          <div>
            <p class="text-sm uppercase tracking-[0.3em] text-slate-500">Start</p>
            <h2 class="text-2xl font-black">Create wallet</h2>
          </div>
          <span class="theme-badge-accent rounded-full px-3 py-1 text-xs font-black">NEW</span>
        </div>
        <form data-action="create-wallet" class="space-y-4">
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Wallet name</span><input class="field" name="name" placeholder="Primary Vault" required /></label>
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Passphrase</span><input class="field" name="passphrase" type="password" minlength="8" placeholder="Minimum 8 characters" data-passphrase-input required /></label>
          ${passphraseMeter()}
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Confirm passphrase</span><input class="field" name="confirmPassphrase" type="password" minlength="8" required /></label>
          <button class="btn-primary w-full" type="submit">Generate wallet</button>
        </form>
        <div class="my-7 h-px bg-white/10"></div>
        <form data-action="import-wallet" class="space-y-4">
          <h3 class="font-black">Import existing wallet</h3>
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Recovery phrase</span><textarea class="field min-h-28" name="mnemonic" placeholder="12 or 24 word phrase" required></textarea></label>
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">New local passphrase</span><input class="field" name="passphrase" type="password" minlength="8" data-passphrase-input required /></label>
          ${passphraseMeter()}
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Confirm passphrase</span><input class="field" name="confirmPassphrase" type="password" minlength="8" required /></label>
          <button class="btn-secondary w-full" type="submit">Import wallet</button>
        </form>
      </div>
    </section>
  `;
}
