import { get } from "svelte/store";
import { watchWorkspaceEvents, type WorkspaceEvent } from "$lib/api";
import { getToken } from "$lib/auth";
import { refreshApps, appsStore } from "./apps";
import { refreshVolumes, refreshSnapshots, volumesStore } from "./volumes";
import { refreshVms } from "./vms";
import { refreshProfile } from "./profile";
import { refreshSecurityRules } from "./networking";

let cleanup: (() => void) | null = null;
let currentToken: string | null = null;

export function initWorkspaceSSE() {
  const token = getToken();
  if (!token) {
    if (cleanup) {
      cleanup();
      cleanup = null;
    }
    currentToken = null;
    return;
  }

  if (token === currentToken && cleanup) return;

  if (cleanup) cleanup();
  currentToken = token;

  cleanup = watchWorkspaceEvents(token, (event: WorkspaceEvent) => {
    console.debug("Received workspace event:", event);

    switch (event.kind) {
      case "app_created":
      case "app_updated":
      case "app_deleted":
        void refreshApps();
        break;

      case "profile_updated":
      case "github_accounts_changed":
        void refreshProfile();
        break;

      case "security_rules_changed":
        if (event.app_name) {
          void refreshSecurityRules(event.app_name);
        }
        break;

      case "deployment_changed":
        void refreshVms();
        // Also refresh apps to update active_deployment_id
        void refreshApps();
        break;

      case "volume_changed": {
        // If we have a specific app_id, we could be more targeted,
        // but for now, if the volumesStore is not empty, we refresh.
        // In a real app, we might want to know which app is currently selected in the UI.
        const apps = get(appsStore);
        if (event.app_id) {
           void refreshVolumes(event.app_id);
        } else {
           // Fallback if app_id is missing: refresh if we have volumes loaded
           const currentVolumes = get(volumesStore);
           if (currentVolumes.length > 0 && currentVolumes[0].app_id) {
             void refreshVolumes(currentVolumes[0].app_id);
           }
        }
        break;
      }

      case "snapshot_changed": {
        if (event.volume_id) {
          void refreshSnapshots(event.volume_id);
        }
        break;
      }

      case "refresh":
        void refreshApps();
        void refreshVms();
        break;

      default:
        break;
    }
  });
}

export function closeWorkspaceSSE() {
  if (cleanup) {
    cleanup();
    cleanup = null;
  }
  currentToken = null;
}

if (typeof window !== "undefined") {
  window.addEventListener("mikrom-auth-change", () => {
    initWorkspaceSSE();
  });
}
