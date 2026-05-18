import { writable } from "svelte/store";
import { listVolumes, listVolumeSnapshots, type Volume, type VolumeSnapshot } from "$lib/api";
import { getToken } from "$lib/auth";

export const volumesStore = writable<Volume[]>([]);
export const snapshotsStore = writable<VolumeSnapshot[]>([]);
export const volumesLoading = writable<boolean>(false);
export const snapshotsLoading = writable<boolean>(false);

export async function refreshVolumes(appId: string) {
  const token = getToken();
  if (!token || !appId) return;

  volumesLoading.set(true);
  try {
    const result = await listVolumes(token, appId);
    if (result.data) {
      volumesStore.set(result.data);
    }
  } finally {
    volumesLoading.set(false);
  }
}

export async function refreshSnapshots(volumeId: string) {
  const token = getToken();
  if (!token || !volumeId) return;

  snapshotsLoading.set(true);
  try {
    const result = await listVolumeSnapshots(token, volumeId);
    if (result.data) {
      snapshotsStore.set(result.data);
    }
  } finally {
    snapshotsLoading.set(false);
  }
}
