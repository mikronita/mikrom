import { writable } from "svelte/store";
import { listSecurityRules, type SecurityRule } from "$lib/api";
import { getToken } from "$lib/auth";

export const securityRulesStore = writable<SecurityRule[]>([]);
export const securityRulesLoading = writable<boolean>(false);

export function clearSecurityRules() {
  securityRulesStore.set([]);
  securityRulesLoading.set(false);
}

export async function refreshSecurityRules(appName: string) {
  const token = getToken();
  if (!token || !appName) return;

  securityRulesLoading.set(true);
  try {
    const result = await listSecurityRules(token, appName);
    if (result.data) {
      securityRulesStore.set(result.data);
    }
  } finally {
    securityRulesLoading.set(false);
  }
}
