import { onDestroy, onMount } from "svelte";
import { getToken } from "$lib/auth";
import { listVms, watchVmsSSE, type LiveDeploymentInfo } from "$lib/api";

let watchers = 0;
const callbacks = new Set<(vms: LiveDeploymentInfo[]) => void>();
let current: LiveDeploymentInfo[] = [];
let cleanup: (() => void) | null = null;

function notify() {
  for (const cb of callbacks) cb(current);
}

export function getCurrentVms() {
  return current;
}

export function setCurrentVms(vms: LiveDeploymentInfo[]) {
  current = vms;
  notify();
}

export async function refreshVms() {
  const token = getToken();
  if (!token) return;
  const result = await listVms(token);
  if (result.data) {
    current = result.data;
    notify();
  }
}

export function useWatchVms() {
  onMount(() => {
    watchers += 1;
    const token = getToken();
    let localCleanup: (() => void) | null = null;

    if (token) {
      void refreshVms();
      localCleanup = watchVmsSSE(token, (updatedVm) => {
        const index = current.findIndex(
          (vm) => vm.deployment_id === updatedVm.deployment_id || (vm.job_id === updatedVm.job_id && vm.job_id !== "")
        );
        const isRunning = updatedVm.status.toLowerCase() === "running";
        if (!isRunning) {
          if (index !== -1) current = current.filter((_, itemIndex) => itemIndex !== index);
        } else if (index === -1) {
          current = [...current, updatedVm];
        } else {
          const next = [...current];
          next[index] = { ...next[index], ...updatedVm };
          current = next;
        }
        notify();
      });
    }

    return () => {
      watchers -= 1;
      if (localCleanup) localCleanup();
      if (watchers <= 0 && cleanup) {
        cleanup();
        cleanup = null;
      }
    };
  });
}

export function subscribeVms(cb: (vms: LiveDeploymentInfo[]) => void) {
  callbacks.add(cb);
  cb(current);
  return () => callbacks.delete(cb);
}
