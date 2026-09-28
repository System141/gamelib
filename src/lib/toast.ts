// Minimal toast store (no provider needed).

import { useSyncExternalStore } from "react";

export type ToastTone = "success" | "info" | "warning" | "error";

export interface Toast {
  id: number;
  tone: ToastTone;
  title: string;
  description?: string;
  /** A button in the toast, e.g. "Geri al". */
  action?: { label: string; onClick: () => void };
}

let toasts: Toast[] = [];
let nextId = 1;
const listeners = new Set<() => void>();

function emit() {
  for (const l of listeners) l();
}

export function dismissToast(id: number) {
  toasts = toasts.filter((t) => t.id !== id);
  emit();
}

export function showToast(toast: Omit<Toast, "id">, durationMs = 5000): number {
  const id = nextId++;
  toasts = [...toasts.slice(-3), { ...toast, id }];
  emit();
  window.setTimeout(() => dismissToast(id), durationMs);
  return id;
}

export function useToasts(): Toast[] {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => toasts,
  );
}
