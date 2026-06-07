import { get } from "svelte/store";
import { watchWorkspaceEvents, type WorkspaceEvent } from "$lib/api";
import { getToken } from "$lib/auth";
import { refreshApps } from "./apps";
import { refreshVolumes, refreshSnapshots, volumesStore } from "./volumes";
import { refreshVms } from "./vms";
import { refreshProfile } from "./profile";
import { refreshBilling } from "./billing";
import { refreshSecurityRules } from "./networking";
import { refreshDatabases } from "./databases";

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

      case "billing_updated":
        void refreshBilling();
        break;

      case "security_rules_changed":
        if (event.app_name) {
          void refreshSecurityRules(event.app_name);
        }
        break;

      case "deployment_changed":
        void refreshVms();
        break;

      case "volume_changed": {
        const currentVolumes = get(volumesStore);
        if (event.app_id && currentVolumes.length > 0 && "mount_point" in currentVolumes[0]) {
          void refreshVolumes(event.app_id);
        } else {
          void refreshVolumes();
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
        void refreshDatabases();
        void refreshBilling();
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
