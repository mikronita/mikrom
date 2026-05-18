import { toast as sonnerToast } from "svelte-sonner";

export const toast = {
  success(message: string, options?: any) {
    return sonnerToast.success(message, options);
  },
  error(message: string, options?: any) {
    return sonnerToast.error(message, options);
  },
  info(message: string, options?: any) {
    return sonnerToast.info(message, options);
  },
  loading(message: string, options?: any) {
    return sonnerToast.loading(message, options);
  },
  message(message: string, options?: any) {
    return sonnerToast.message(message, options);
  },
  dismiss(id?: string | number) {
    return sonnerToast.dismiss(id);
  },
  promise: sonnerToast.promise,
};
