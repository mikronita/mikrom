import { writable } from "svelte/store";
import { listApps, type AppInfo, type LiveDeploymentInfo } from "$lib/api";
import { getToken } from "$lib/auth";

export const appsStore = writable<AppInfo[]>([]);
export const appsLoading = writable<boolean>(false);
export const appsError = writable<string>("");

export function clearApps() {
  appsStore.set([]);
  appsLoading.set(false);
  appsError.set("");
}

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

export function applyDeploymentUpdate(deployment: LiveDeploymentInfo) {
  const deploymentKey = deployment.deployment_id || deployment.job_id || null;
  const normalizedStatus = deployment.status.toLowerCase();
  const isActive = !["stopped", "failed", "cancelled", "error"].includes(normalizedStatus);

  appsStore.update((current) =>
    current.map((app) => {
      if (app.id !== deployment.app_id && app.name !== deployment.app_name) {
        return app;
      }

      const nextActiveDeploymentId =
        isActive && deploymentKey
          ? deploymentKey
          : app.active_deployment_id === deploymentKey
            ? null
            : app.active_deployment_id;

      return {
        ...app,
        active_deployment_id: nextActiveDeploymentId,
        scale_state: deployment.scale_state ?? app.scale_state,
      };
    }),
  );
}
