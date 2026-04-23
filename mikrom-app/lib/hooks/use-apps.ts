"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { 
  listApps, 
  createApp, 
  deleteApp,
  deployAppVersion, 
  listDeployments, 
  activateDeployment,
  CreateAppRequest,
  DeployRequest 
} from "@/lib/api";
import { getToken } from "@/lib/auth";

export const appsKeys = {
  all: ["apps"] as const,
  list: () => [...appsKeys.all, "list"] as const,
  detail: (id: string) => [...appsKeys.all, "detail", id] as const,
  deployments: (appId: string) => [...appsKeys.all, "deployments", appId] as const,
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
    },
  });
}

export function useDeleteApp() {
  const queryClient = useQueryClient();
  const token = getToken();

  return useMutation({
    mutationFn: async (appId: string) => {
      if (!token) throw new Error("No token found");
      const result = await deleteApp(token, appId);
      if (result.error) throw new Error(result.error);
      return result.success;
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: appsKeys.list() });
    },
  });
}

export function useDeployAppVersion() {
  const queryClient = useQueryClient();
  const token = getToken();

  return useMutation({
    mutationFn: async ({ appId, data }: { appId: string, data?: Partial<DeployRequest> }) => {
      if (!token) throw new Error("No token found");
      const result = await deployAppVersion(token, appId, data);
      if (result.error) throw new Error(result.error);
      return result.data;
    },
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({ queryKey: appsKeys.deployments(variables.appId) });
      queryClient.invalidateQueries({ queryKey: ["vms"] });
    },
  });
}

export function useDeployments(appId: string) {
  const token = getToken();

  return useQuery({
    queryKey: appsKeys.deployments(appId),
    queryFn: async () => {
      if (!token) throw new Error("No token found");
      const result = await listDeployments(token, appId);
      if (result.error) throw new Error(result.error);
      return result.data ?? [];
    },
    enabled: !!token && !!appId,
    refetchInterval: 5000,
  });
}

export function useActivateDeployment() {
  const queryClient = useQueryClient();
  const token = getToken();

  return useMutation({
    mutationFn: async ({ appId, deploymentId }: { appId: string, deploymentId: string }) => {
      if (!token) throw new Error("No token found");
      const result = await activateDeployment(token, appId, deploymentId);
      if (result.error) throw new Error(result.error);
      return result.success;
    },
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({ queryKey: appsKeys.list() });
      queryClient.invalidateQueries({ queryKey: appsKeys.deployments(variables.appId) });
    },
  });
}
