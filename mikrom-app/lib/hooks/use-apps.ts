"use client";

import { useEffect } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { 
  listApps, 
  createApp, 
  deleteApp,
  deployAppVersion, 
  listDeployments, 
  activateDeployment,
  CreateAppRequest,
  DeployRequest,
  API_BASE_URL
} from "@/lib/api";
import { getToken } from "@/lib/auth";

export const appsKeys = {
  all: ["apps"] as const,
  list: () => [...appsKeys.all, "list"] as const,
  detail: (id: string) => [...appsKeys.all, "detail", id] as const,
  deployments: (appName: string) => [...appsKeys.all, "deployments", appName] as const,
};

export function useApps() {
  const token = getToken();

  return useQuery({
    queryKey: appsKeys.list(),
    queryFn: async () => {
      if (!token) throw new Error("No token found");
      const result = await listApps(token);
      if (result.error) throw new Error(result.error);
      return result.data ?? [];
    },
    enabled: !!token,
  });
}

export function useCreateApp() {
  const queryClient = useQueryClient();
  const token = getToken();

  return useMutation({
    mutationFn: async (data: CreateAppRequest) => {
      if (!token) throw new Error("No token found");
      const result = await createApp(token, data);
      if (result.error) throw new Error(result.error);
      return result.data;
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: appsKeys.list() });
      queryClient.invalidateQueries({ queryKey: ["vms"] });
    },
  });
}

export function useDeleteApp() {
  const queryClient = useQueryClient();
  const token = getToken();

  return useMutation({
    mutationFn: async (appName: string) => {
      if (!token) throw new Error("No token found");
      const result = await deleteApp(token, appName);
      if (result.error) throw new Error(result.error);
      return result.success;
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: appsKeys.list() });
      queryClient.invalidateQueries({ queryKey: ["vms"] });
    },
  });
}

export function useDeployAppVersion() {
  const queryClient = useQueryClient();
  const token = getToken();

  return useMutation({
    mutationFn: async ({ appName, data }: { appName: string, data?: Partial<DeployRequest> }) => {
      if (!token) throw new Error("No token found");
      const result = await deployAppVersion(token, appName, data);
      if (result.error) throw new Error(result.error);
      return result.data;
    },
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({ queryKey: appsKeys.deployments(variables.appName) });
      queryClient.invalidateQueries({ queryKey: ["vms"] });
    },
  });
}

export function useDeployments(appName: string) {
  const token = getToken();
  const queryClient = useQueryClient();

  useEffect(() => {
    if (!token || !appName) return;

    const eventSource = new EventSource(`${API_BASE_URL}/apps/${appName}/deployments/stream?token=${token}`);

    eventSource.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        queryClient.setQueryData(appsKeys.deployments(appName), data);
        // Also invalidate the apps list to update active_deployment_id and other metadata
        queryClient.invalidateQueries({ queryKey: appsKeys.list() });
      } catch (err) {
        console.error("Failed to parse SSE data", err);
      }
    };

    eventSource.onerror = () => {
      // EventSource automatically reconnects, but we log the error
      console.debug("SSE connection error, attempting to reconnect...");
    };

    return () => {
      eventSource.onmessage = null;
      eventSource.onerror = null;
      eventSource.close();
    };
  }, [appName, token, queryClient]);

  return useQuery({
    queryKey: appsKeys.deployments(appName),
    queryFn: async () => {
      if (!token) throw new Error("No token found");
      const result = await listDeployments(token, appName);
      if (result.error) throw new Error(result.error);
      return result.data ?? [];
    },
    enabled: !!token && !!appName,
  });
}

export function useActivateDeployment() {
  const queryClient = useQueryClient();
  const token = getToken();

  return useMutation({
    mutationFn: async ({ appName, deploymentId }: { appName: string, deploymentId: string }) => {
      if (!token) throw new Error("No token found");
      const result = await activateDeployment(token, appName, deploymentId);
      if (result.error) throw new Error(result.error);
      return result.success;
    },
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({ queryKey: appsKeys.list() });
      queryClient.invalidateQueries({ queryKey: appsKeys.deployments(variables.appName) });
    },
  });
}
