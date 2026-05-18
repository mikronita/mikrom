import { writable } from "svelte/store";
import { listApps, type AppInfo } from "$lib/api";
import { getToken } from "$lib/auth";

export const appsStore = writable<AppInfo[]>([]);
export const appsLoading = writable<boolean>(false);
export const appsError = writable<string>("");

export async function refreshApps() {
  const token = getToken();
  if (!token) return;

  appsLoading.set(true);
  try {
    const result = await listApps(token);
    if (result.error) {
      appsError.set(result.error);
    } else if (result.data) {
      appsStore.set(result.data);
      appsError.set("");
    }
  } catch (e) {
    appsError.set(e instanceof Error ? e.message : "Failed to fetch apps");
  } finally {
    appsLoading.set(false);
  }
}
