import type { Toast } from "./types";
import { toastTemplate } from "./views/toast";

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

  toastRoot.innerHTML = toasts
    .map((toast) => toastTemplate(toast, enteredToasts.has(toast.id)))
    .join("");

  toastRoot.querySelectorAll<HTMLElement>("[data-toast-id]").forEach((element) => {
    const id = Number(element.dataset.toastId);
    const previousTop = previousTops.get(id);
    const nextTop = element.getBoundingClientRect().top;

    if (
      previousTop !== undefined &&
      previousTop !== nextTop &&
      !element.classList.contains("toast-exit")
    ) {
      element.animate(
        [{ transform: `translateY(${previousTop - nextTop}px)` }, { transform: "translateY(0)" }],
        {
          duration: 260,
          easing: "cubic-bezier(.2, .9, .2, 1)",
        },
      );
    }

    enteredToasts.add(id);
  });
}
