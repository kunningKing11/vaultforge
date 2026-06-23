import { escapeHtml } from "./format";
import type { Toast } from "./types";

let toastId = 0;
let toasts: Toast[] = [];
const enteredToasts = new Set<number>();

const toastRoot = document.createElement("div");
toastRoot.className = "toast-stack";
document.body.appendChild(toastRoot);

export function pushToast(message: string, tone: Toast["tone"]) {
  const toast: Toast = {
    id: ++toastId,
    message,
    tone,
    createdAt: Date.now(),
    duration: 4_500,
    exiting: false,
  };

  toasts = [...toasts, toast];
  renderToasts();
  window.setTimeout(() => dismissToast(toast.id), toast.duration);
}

function dismissToast(id: number) {
  const toast = toasts.find((item) => item.id === id);
  if (!toast || toast.exiting) return;

  toasts = toasts.map((item) => (item.id === id ? { ...item, exiting: true } : item));
  renderToasts();
  window.setTimeout(() => {
    toasts = toasts.filter((item) => item.id !== id);
    enteredToasts.delete(id);
    renderToasts();
  }, 240);
}

function renderToasts() {
  const previousTops = new Map<number, number>();
  toastRoot.querySelectorAll<HTMLElement>("[data-toast-id]").forEach((element) => {
    previousTops.set(Number(element.dataset.toastId), element.getBoundingClientRect().top);
  });

  toastRoot.innerHTML = toasts.map(toastHtml).join("");

  toastRoot.querySelectorAll<HTMLElement>("[data-toast-id]").forEach((element) => {
    const id = Number(element.dataset.toastId);
    const previousTop = previousTops.get(id);
    const nextTop = element.getBoundingClientRect().top;

    if (previousTop !== undefined && previousTop !== nextTop && !element.classList.contains("toast-exit")) {
      element.animate([{ transform: `translateY(${previousTop - nextTop}px)` }, { transform: "translateY(0)" }], {
        duration: 260,
        easing: "cubic-bezier(.2, .9, .2, 1)",
      });
    }

    enteredToasts.add(id);
  });
}

function toastHtml(toast: Toast) {
  const elapsed = Date.now() - toast.createdAt;
  const entryClass = enteredToasts.has(toast.id) || toast.exiting ? "" : "toast-enter";
  const exitClass = toast.exiting ? "toast-exit" : "";
  const toneClass = toast.tone === "error" ? "toast-error" : "toast-success";

  return `
    <article class="toast-card ${toneClass} ${entryClass} ${exitClass}" data-toast-id="${toast.id}">
      <div class="flex items-start gap-3">
        <div class="toast-dot"></div>
        <p class="text-sm font-bold leading-6">${escapeHtml(toast.message)}</p>
      </div>
      <div class="toast-track"><div class="toast-progress" style="animation-duration: ${toast.duration}ms; animation-delay: -${Math.min(elapsed, toast.duration)}ms;"></div></div>
    </article>
  `;
}
