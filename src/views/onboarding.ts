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
            <h2 class="text-2xl font-black">Create wallet</h2>
          </div>
        </div>
        <form data-action="wallet-setup" class="space-y-4">
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Wallet name</span><input class="field" name="name" placeholder="Primary Vault" required /></label>
          <hr class="my-4 border-white/10" />
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Passphrase</span><input class="field" name="passphrase" type="password" minlength="8" placeholder="Minimum 8 characters" data-passphrase-input required /></label>
          ${passphraseMeter()}
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Confirm passphrase</span><input class="field" name="confirmPassphrase" type="password" minlength="8" required /></label>
          <hr class="my-4 border-white/10" />
          <h3 class="font-black">Import an existing wallet</h3>
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Recovery phrase</span><textarea class="field min-h-28" name="mnemonic" placeholder="12 or 24 word phrase (leave blank to create new)"></textarea></label>
          <button class="btn-primary w-full" type="submit">Generate wallet</button>
        </form>
      </div>
    </section>
  `;
}
