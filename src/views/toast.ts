import { escapeHtml } from "../format";
import type { Toast } from "../types";

export function toastTemplate(toast: Toast, entered: boolean) {
  const elapsed = Date.now() - toast.createdAt;
  const entryClass = entered || toast.exiting ? "" : "toast-enter";
  const exitClass = toast.exiting ? "toast-exit" : "";
  const toneClass = `toast-${toast.tone}`;

  return `
    <article class="toast-card ${toneClass} ${entryClass} ${exitClass}" data-toast-id="${toast.id}">
      <div class="flex items-start gap-3">
        <div class="toast-dot"></div>
        <p class="text-sm font-bold font-bold leading-6">${escapeHtml(toast.message)}</p>
      </div>
      <div class="toast-track"><div class="toast-progress" style="animation-duration: ${toast.duration}ms; animation-delay: -${Math.min(elapsed, toast.duration)}ms;"></div></div>
    </article>
  `;
}
