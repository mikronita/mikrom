import { writable } from "svelte/store";
import { health } from "$lib/api";

export interface HealthStatus {
  status: string;
  version: string;
  services: Record<string, string>;
}

export const healthStore = writable<HealthStatus | null>(null);
export const healthLoading = writable(false);

let healthInterval: ReturnType<typeof setInterval> | null = null;

export async function refreshHealth() {
  healthLoading.set(true);
  try {
    const result = await health();
    healthStore.set({
      status: result.status,
      version: result.version,
      services: result.services || {}
    });
  } catch (error) {
    console.error("Failed to fetch health status", error);
  } finally {
    healthLoading.set(false);
  }
}

export function initHealthPolling(intervalMs = 30000) {
  if (healthInterval) return;
  
  refreshHealth();
  healthInterval = setInterval(refreshHealth, intervalMs);
  
  return () => {
    if (healthInterval) {
      clearInterval(healthInterval);
      healthInterval = null;
    }
  };
}
