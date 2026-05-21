import { writable } from "svelte/store";
import { listVolumes, listAllVolumes, listVolumeSnapshots, type Volume, type VolumeSnapshot, type AttachedVolume, type VolumeWithAttachments } from "$lib/api";
import { getToken } from "$lib/auth";

export const volumesStore = writable<Volume[] | AttachedVolume[] | VolumeWithAttachments[]>([]);
export const snapshotsStore = writable<VolumeSnapshot[]>([]);
export const volumesLoading = writable<boolean>(false);
export const snapshotsLoading = writable<boolean>(false);

export async function refreshVolumes(appId?: string) {
  const token = getToken();
  if (!token) return;

  volumesLoading.set(true);
  try {
    const result = appId ? await listVolumes(token, appId) : await listAllVolumes(token);
    if (result.data) {
      volumesStore.set([...result.data]);
    } else {
      volumesStore.set([]);
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
