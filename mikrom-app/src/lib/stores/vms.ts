import { writable } from "svelte/store";
import { getToken } from "$lib/auth";
import { listVms, watchVmsSSE, type LiveDeploymentInfo } from "$lib/api";

export const vmsStore = writable<LiveDeploymentInfo[]>([]);
export const vmsLoading = writable<boolean>(false);

let currentVms: LiveDeploymentInfo[] = [];
vmsStore.subscribe(value => {
  currentVms = value;
});

export function getCurrentVms() {
  return currentVms;
}

export async function refreshVms() {
  const token = getToken();
  if (!token) return;

  vmsLoading.set(true);
  try {
    const result = await listVms(token);
    if (result.data) {
      vmsStore.set(result.data);
    }
  } finally {
    vmsLoading.set(false);
  }
}

let sseCleanup: (() => void) | null = null;

export function initVmsSSE() {
  const token = getToken();
  if (!token) return;

  if (sseCleanup) sseCleanup();

  sseCleanup = watchVmsSSE(token, (updatedVm) => {
    vmsStore.update(current => {
      // Prioritize matching by Job ID as it is the unique identifier for a running instance
      const index = current.findIndex(
        (vm) => (updatedVm.job_id !== "" && vm.job_id === updatedVm.job_id) || 
                (updatedVm.job_id === "" && vm.deployment_id === updatedVm.deployment_id)
      );
      
      const isRunning = updatedVm.status.toLowerCase() === "running";
      
      if (!isRunning) {
        // If it's no longer running, remove it from the active VMs list
        if (index !== -1) return current.filter((_, itemIndex) => itemIndex !== index);
        return current;
      } else if (index === -1) {
        // New running VM
        return [...current, updatedVm];
      } else {
        // Update existing VM status/metrics
        const next = [...current];
        next[index] = { ...next[index], ...updatedVm };
        return next;
      }
    });
  });
}

export function stopVmsSSE() {
  if (sseCleanup) {
    sseCleanup();
    sseCleanup = null;
  }
}

if (typeof window !== "undefined") {
  window.addEventListener("mikrom-auth-change", () => {
    initVmsSSE();
  });
}

// Deprecated in favor of store subscriptions
export function subscribeVms(cb: (vms: LiveDeploymentInfo[]) => void) {
  return vmsStore.subscribe(cb);
}
