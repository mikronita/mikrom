import { writable } from "svelte/store";
import { health, watchHealthSSE } from "$lib/api";

export interface HealthStatus {
  status: string;
  version: string;
  services: Record<string, string>;
}

export const healthStore = writable<HealthStatus | null>(null);
export const healthLoading = writable(false);

let healthStreamCleanup: (() => void) | null = null;

function cleanupHealthStream() {
  if (!healthStreamCleanup) return;
  healthStreamCleanup();
  healthStreamCleanup = null;
}

function setHealthStatus(result: { status: string; version: string; services?: Record<string, string> }) {
  healthStore.set({
    status: result.status,
    version: result.version,
    services: result.services || {},
  });
}

export async function refreshHealth() {
  healthLoading.set(true);
  try {
    const result = await health();
    setHealthStatus(result);
  } catch (error) {
    console.error("Failed to fetch health status", error);
  } finally {
    healthLoading.set(false);
  }
}

export function initHealthStream() {
  if (healthStreamCleanup) return cleanupHealthStream;

  void refreshHealth();

  healthStreamCleanup = watchHealthSSE((result) => {
    setHealthStatus(result);
    healthLoading.set(false);
  });

  return cleanupHealthStream;
}

export function initHealthPolling() {
  return initHealthStream();
}
