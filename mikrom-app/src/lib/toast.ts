import { toast as sonnerToast } from "svelte-sonner";

type ToastOptions = Parameters<typeof sonnerToast.success>[1];

export const toast = {
  success(message: string, options?: ToastOptions) {
    return sonnerToast.success(message, options);
  },
  error(message: string, options?: ToastOptions) {
    return sonnerToast.error(message, options);
  },
  info(message: string, options?: ToastOptions) {
    return sonnerToast.info(message, options);
  },
  loading(message: string, options?: ToastOptions) {
    return sonnerToast.loading(message, options);
  },
  message(message: string, options?: ToastOptions) {
    return sonnerToast.message(message, options);
  },
  dismiss(id?: string | number) {
    return sonnerToast.dismiss(id);
  },
  promise: sonnerToast.promise,
};
