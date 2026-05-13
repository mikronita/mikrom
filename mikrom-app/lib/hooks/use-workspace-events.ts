"use client";

import { useCallback, useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { useAuthToken } from "@/lib/hooks/use-auth-token";
import { watchWorkspaceEvents, type WorkspaceEvent } from "@/lib/api";
import { appsKeys } from "@/lib/hooks/use-apps";
import { vmsKeys } from "@/lib/hooks/use-vms";

export function useWorkspaceEvents() {
  const queryClient = useQueryClient();
  const token = useAuthToken();
  const invalidateAllWorkspaceState = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: appsKeys.all });
    queryClient.invalidateQueries({ queryKey: vmsKeys.all });
    queryClient.invalidateQueries({ queryKey: ["active-deployments"] });
    queryClient.invalidateQueries({ queryKey: ["profile"] });
    queryClient.invalidateQueries({ queryKey: ["github-accounts"] });
    queryClient.invalidateQueries({ queryKey: ["github", "repos"] });
    queryClient.invalidateQueries({ queryKey: ["security-rules"] });
  }, [queryClient]);

  useEffect(() => {
    if (!token) return;

    const invalidateWorkspaceState = (event: WorkspaceEvent) => {
      switch (event.kind) {
        case "app_created":
        case "app_updated":
        case "app_deleted":
        case "deployment_changed":
          invalidateAllWorkspaceState();
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
        case "refresh":
          invalidateAllWorkspaceState();
          break;
        default:
          break;
      }
    };

    const cleanup = watchWorkspaceEvents(token, invalidateWorkspaceState);
    return () => cleanup();
  }, [invalidateAllWorkspaceState, queryClient, token]);
}
