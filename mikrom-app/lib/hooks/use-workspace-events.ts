"use client";

import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { getToken } from "@/lib/auth";
import { watchWorkspaceEvents, type WorkspaceEvent } from "@/lib/api";
import { appsKeys } from "@/lib/hooks/use-apps";
import { vmsKeys } from "@/lib/hooks/use-vms";

export function useWorkspaceEvents() {
  const queryClient = useQueryClient();
  const token = getToken();

  useEffect(() => {
    if (!token) return;

    const invalidateWorkspaceState = (event: WorkspaceEvent) => {
      switch (event.kind) {
        case "app_created":
        case "app_updated":
        case "app_deleted":
        case "deployment_changed":
          queryClient.invalidateQueries({ queryKey: appsKeys.all });
          queryClient.invalidateQueries({ queryKey: vmsKeys.all });
          queryClient.invalidateQueries({ queryKey: ["active-deployments"] });
          break;
        case "profile_updated":
          queryClient.invalidateQueries({ queryKey: ["profile"] });
          break;
        case "github_accounts_changed":
          queryClient.invalidateQueries({ queryKey: ["github-accounts"] });
          queryClient.invalidateQueries({ queryKey: ["github", "repos"] });
          break;
        case "security_rules_changed":
          queryClient.invalidateQueries({ queryKey: ["security-rules"] });
          break;
        default:
          break;
      }
    };

    const cleanup = watchWorkspaceEvents(token, invalidateWorkspaceState);
    return () => cleanup();
  }, [queryClient, token]);
}
