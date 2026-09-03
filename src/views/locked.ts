import eyeOffIcon from "../assets/icons/eye-off.svg?raw";
import eyeIcon from "../assets/icons/eye.svg?raw";
import { appState } from "../state";
import { inlineIcon } from "./shared";

export function lockedWalletView() {
  const revealLabel = appState.dialogs.unlockPasswordVisible
    ? "Hide wallet password"
    : "Show wallet password";

  const passwordIcon = appState.dialogs.unlockPasswordVisible ? eyeOffIcon : eyeIcon;

  return `
    <section class="mx-auto flex min-h-[88vh] max-w-xl items-center justify-center">
      <div class="glass w-full rounded-[2rem] p-8 text-center">
        <div class="theme-icon-accent mx-auto mb-5 flex h-16 w-16 items-center justify-center rounded-2xl text-3xl">#</div>
        <p class="text-sm font-bold uppercase tracking-[0.3em] text-slate-500">Wallet locked</p>
        <h1 class="mt-2 text-3xl font-black">Unlock Wallet</h1>
        <p class="mt-3 text-slate-400">Your wallet session is encrypted locally. Enter your wallet password to unlock your wallet.</p>
        <form data-action="unlock-wallet" class="mt-7 space-y-4 text-left">
          <label class="block space-y-2">
            <span class="text-sm font-bold font-bold text-slate-300">Wallet password</span>
            <div class="relative">
              <input class="field pr-12" name="walletPassword" type="${appState.dialogs.unlockPasswordVisible ? "text" : "password"}" required />
              <button class="absolute right-3 top-1/2 -translate-y-1/2" type="button" data-action="toggle-unlock-password-visibility" aria-label="${revealLabel}" aria-pressed="${appState.dialogs.unlockPasswordVisible}">
                ${inlineIcon({ svg: passwordIcon })}
              </button>
            </div>
          </label>
          <button class="btn-primary w-full" type="submit">Unlock wallet</button>
        </form>
        <div class="mt-7 border-t border-rose-400/20 pt-5 text-left">
          <button class="btn-danger w-full" data-action="show-locked-delete-wallet" type="button">Delete stored wallet</button>
          <p class="mt-3 text-sm font-bold leading-6 text-rose-200/80">Only use this if you need to remove the encrypted local wallet from this device and your seed is backed up.</p>
        </div>
      </div>
    </section>
  `;
}

export function deleteWalletModal() {
  if (appState.dialogs.deleteWallet.step === "idle") return "";

  if (appState.dialogs.deleteWallet.step === "confirm") {
    return `
      <div class="destructive-modal fixed inset-0 z-[70] flex items-center justify-center bg-slate-950/75 p-4 backdrop-blur-md">
        <section class="w-full max-w-2xl rounded-[2rem] border border-white/10 bg-slate-950/95 p-6 text-left text-slate-100 shadow-[0_30px_120px_rgba(2,6,23,0.72)] sm:p-8">
          <p class="text-xs font-black uppercase tracking-[0.3em] text-rose-300">Destructive Action</p>
          <h2 class="mt-3 text-3xl font-black text-white">Are you sure you want to do this?</h2>
          <p class="mt-4 text-sm font-bold leading-6 text-slate-300">Deleting stored wallet files is destructive. You could lose all your funds if your wallet seed is not backed up.</p>
          <div class="mt-6 rounded-2xl border border-rose-400/25 bg-rose-400/10 p-4 text-sm font-bold font-bold leading-6 text-rose-100">Only continue if you have verified your recovery phrase is backed up and usable.</div>
          <div class="mt-6 flex flex-col gap-3 sm:flex-row">
            <button class="btn-secondary flex-1" data-action="cancel-locked-delete-wallet" type="button">Cancel</button>
            <button class="btn-danger flex-1 whitespace-nowrap" data-action="start-locked-delete-wallet-countdown" type="button">Yes, I have backed up my wallet</button>
          </div>
        </section>
      </div>
    `;
  }

  return `
    <div class="destructive-modal fixed inset-0 z-[70] flex items-center justify-center bg-slate-950/75 p-4 backdrop-blur-md">
      <section class="w-full max-w-md rounded-[2rem] border border-rose-400/50 bg-rose-950/90 p-6 text-center text-rose-100 shadow-[0_30px_120px_rgba(127,29,29,0.55)] sm:p-8">
        <p class="text-xs font-black uppercase tracking-[0.3em] text-rose-300">Deletion Pending</p>
        <h2 class="mt-3 text-2xl font-black text-rose-50">Deleting wallet files in</h2>
        <p class="mt-5 text-7xl font-black text-rose-50">${appState.dialogs.deleteWallet.secondsRemaining}</p>
        <p class="mt-5 text-sm font-bold leading-6 text-rose-100/80">This will permanently remove the encrypted local wallet from this device.</p>
        <button class="btn-secondary mt-6 w-full" data-action="cancel-locked-delete-wallet" type="button">Cancel</button>
      </section>
    </div>
  `;
}
