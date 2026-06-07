import { onMount } from "svelte";
import { writable } from "svelte/store";
import { getBillingSummary, type BillingSummary } from "$lib/api";
import { getToken } from "$lib/auth";

export const billingStore = writable<BillingSummary | null>(null);
export const billingLoading = writable(false);
export const billingError = writable("");

export async function refreshBilling() {
  const token = getToken();
  if (!token) {
    billingStore.set(null);
    billingError.set("");
    billingLoading.set(false);
    return;
  }

  billingLoading.set(true);
  try {
    const result = await getBillingSummary(token);
    if (result.error) {
      billingError.set(result.error);
      billingStore.set(null);
      return;
    }

    billingStore.set(result.data ?? null);
    billingError.set("");
  } catch (error) {
    billingError.set(error instanceof Error ? error.message : "Failed to fetch billing");
  } finally {
    billingLoading.set(false);
  }
}

export const billing = {
  subscribe: billingStore.subscribe,
};

export function useBillingBootstrap() {
  onMount(() => {
    void refreshBilling();

    const handleAuthChange = () => {
      void refreshBilling();
    };

    const handleProjectChange = () => {
      void refreshBilling();
    };

    window.addEventListener("mikrom-auth-change", handleAuthChange);
    window.addEventListener("mikrom-project-change", handleProjectChange);

    return () => {
      window.removeEventListener("mikrom-auth-change", handleAuthChange);
      window.removeEventListener("mikrom-project-change", handleProjectChange);
    };
  });
}
