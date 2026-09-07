// Transient toast notifications into the #toasts host from index.html.
// "error" uses the alarm styling, "info" the muted one.

const TOAST_LIFETIME_MS = 4000;

export type ToastKind = "info" | "error";

export function showToast(message: string, kind: ToastKind = "info"): void {
  const host = document.getElementById("toasts");
  if (!host) {
    console.warn("toast dropped (#toasts missing):", message);
    return;
  }
  const el = document.createElement("div");
  el.className = kind === "error" ? "toast" : "toast info";
  el.textContent = message;
  host.appendChild(el);
  window.setTimeout(() => el.remove(), TOAST_LIFETIME_MS);
}
