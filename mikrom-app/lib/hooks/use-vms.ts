"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { listVms, deployApp, getVm, stopVm, deleteVm, DeployRequest, watchVmsSSE, LiveDeploymentInfo, LiveDeploymentStatus } from "@/lib/api";
import { getToken } from "@/lib/auth";
import { useEffect } from "react";

export const vmsKeys = {
  all: ["vms"] as const,
  list: () => [...vmsKeys.all, "list"] as const,
  detail: (id: string) => [...vmsKeys.all, "detail", id] as const,
};

export function useVms() {
  const token = getToken();

  return useQuery({
    queryKey: vmsKeys.list(),
    queryFn: async () => {
      if (!token) throw new Error("No token found");
      const result = await listVms(token);
      if (result.error) throw new Error(result.error);
      return result.data ?? [];
    },
    enabled: !!token,
    // Polling disabled in favor of SSE (useWatchVms)
    refetchInterval: false,
  });
}

export function useWatchVms() {
  const queryClient = useQueryClient();
  const token = getToken();

  useEffect(() => {
    if (!token) return;
    let isMounted = true;
    const cleanupRef = { current: null as (() => void) | null };

    const startWatching = async () => {
      cleanupRef.current = watchVmsSSE(
        token,
        (updatedVm) => {
          if (!isMounted) return;
          queryClient.setQueryData<LiveDeploymentInfo[]>(vmsKeys.list(), (old = []) => {
            const index = old.findIndex((vm) => 
              vm.deployment_id === updatedVm.deployment_id || 
              (vm.job_id === updatedVm.job_id && vm.job_id !== "")
            );
            if (index === -1) {
              return [...old, updatedVm];
            }
            const next = [...old];
            next[index] = { ...old[index], ...updatedVm };
            return next;
          });

          // Also update detail if it exists
          queryClient.setQueryData<LiveDeploymentStatus>(vmsKeys.detail(updatedVm.job_id), (old) => {
            if (!old) return old;
            return { ...old, ...updatedVm };
          });
        }
      );
    };

    startWatching();

    return () => {
      isMounted = false;
      if (cleanupRef.current) cleanupRef.current();
    };
  }, [token, queryClient]);
}

export function useVm(appName: string, jobId: string) {
  const token = getToken();

  return useQuery({
    queryKey: vmsKeys.detail(jobId),
    queryFn: async () => {
      if (!token) throw new Error("No token found");
      const result = await getVm(token, appName, jobId);
      if (result.error) throw new Error(result.error);
      return result.data;
    },
    enabled: !!token && !!jobId && !!appName,
  });
}

export function useDeployApp() {
  const queryClient = useQueryClient();
  const token = getToken();

  return useMutation({
    mutationFn: async (data: DeployRequest) => {
      if (!token) throw new Error("No token found");
      // Use the application-specific deploy handler
      const result = await deployApp(token, data.app_name, {
        vcpus: data.vcpus,
        memory_mib: data.memory_mib,
        disk_mib: data.disk_mib,
        env: data.env,
        image: data.image
      });
      if (result.error) throw new Error(result.error);
      return result.data;
    },
    onSuccess: () => {
      // Invalidar la lista para forzar un refresh
      queryClient.invalidateQueries({ queryKey: vmsKeys.list() });
    },
  });
}

export function useStopVm() {
  const queryClient = useQueryClient();
  const token = getToken();

  return useMutation({
    mutationFn: async ({ appName, jobId }: { appName: string, jobId: string }) => {
      if (!token) throw new Error("No token found");
      const result = await stopVm(token, appName, jobId);
      if (result.error) throw new Error(result.error);
      return result.data;
    },
    onSuccess: (_, variables) => {
      queryClient.invalidateQueries({ queryKey: vmsKeys.list() });
      queryClient.invalidateQueries({ queryKey: vmsKeys.detail(variables.jobId) });
    },
  });
}

export function useDeleteVm() {
  const queryClient = useQueryClient();
  const token = getToken();

  return useMutation({
    mutationFn: async ({ appName, jobId }: { appName: string, jobId: string }) => {
      if (!token) throw new Error("No token found");
      const result = await deleteVm(token, appName, jobId);
      if (result.error) throw new Error(result.error);
      return result.success;
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: vmsKeys.list() });
    },
  });
}
