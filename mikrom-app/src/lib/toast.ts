import { writable } from "svelte/store";

export type ToastVariant = "success" | "error" | "loading" | "info";

export type ToastItem = {
  id: number;
  message: string;
  variant: ToastVariant;
  duration: number;
};

const DEFAULT_DURATION = 3500;
const nextId = () => Math.floor(Math.random() * 1_000_000_000);

export const toasts = writable<ToastItem[]>([]);

function push(message: string, variant: ToastVariant, duration = DEFAULT_DURATION) {
  const id = nextId();
  const item: ToastItem = { id, message, variant, duration };
  toasts.update((current) => [...current, item]);

  if (variant !== "loading" && duration > 0 && typeof window !== "undefined") {
    window.setTimeout(() => dismiss(id), duration);
  }

  return id;
}

export function dismiss(id?: number) {
  if (typeof id === "number") {
    toasts.update((current) => current.filter((item) => item.id !== id));
    return;
  }
  toasts.set([]);
}

export const toast = {
  success(message: string, duration?: number) {
    return push(message, "success", duration);
  },
  error(message: string, duration?: number) {
    return push(message, "error", duration);
  },
  info(message: string, duration?: number) {
    return push(message, "info", duration);
  },
  loading(message: string) {
    return push(message, "loading", 0);
  },
  dismiss,
  async promise<T>(
    promise: Promise<T>,
    messages: { loading: string; success: string | ((value: T) => string); error: string | ((error: unknown) => string) }
  ) {
    const loadingId = push(messages.loading, "loading", 0);
    try {
      const value = await promise;
      dismiss(loadingId);
      const successMessage = typeof messages.success === "function" ? messages.success(value) : messages.success;
      push(successMessage, "success");
      return value;
    } catch (error) {
      dismiss(loadingId);
      const errorMessage = typeof messages.error === "function" ? messages.error(error) : messages.error;
      push(errorMessage, "error");
      throw error;
    }
  },
};
