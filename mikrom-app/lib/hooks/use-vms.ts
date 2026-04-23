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
            const index = old.findIndex((vm) => vm.job_id === updatedVm.job_id);
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
        },
        (error) => {
          if (isMounted) {
            console.error("VMS SSE Error:", error);
          }
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

export function useVm(jobId: string) {
  const token = getToken();

  return useQuery({
    queryKey: vmsKeys.detail(jobId),
    queryFn: async () => {
      if (!token) throw new Error("No token found");
      const result = await getVm(token, jobId);
      if (result.error) throw new Error(result.error);
      return result.data;
    },
    enabled: !!token && !!jobId,
    refetchInterval: (query) => {
      // Solo refrescar si no está en un estado final
      const status = query.state.data?.status?.toLowerCase();
      if (status === "running" || status === "failed" || status === "cancelled") {
        return 10000; // Más lento si ya está listo o falló
      }
      return 2000; // Rápido si está pending/scheduled
    },
  });
}

export function useDeployApp() {
  const queryClient = useQueryClient();
  const token = getToken();

  return useMutation({
    mutationFn: async (data: DeployRequest) => {
      if (!token) throw new Error("No token found");
      const result = await deployApp(token, data);
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
    mutationFn: async (jobId: string) => {
      if (!token) throw new Error("No token found");
      const result = await stopVm(token, jobId);
      if (result.error) throw new Error(result.error);
      return result.data;
    },
    onSuccess: (_, jobId) => {
      queryClient.invalidateQueries({ queryKey: vmsKeys.list() });
      queryClient.invalidateQueries({ queryKey: vmsKeys.detail(jobId) });
    },
  });
}

export function useDeleteVm() {
  const queryClient = useQueryClient();
  const token = getToken();

  return useMutation({
    mutationFn: async (jobId: string) => {
      if (!token) throw new Error("No token found");
      const result = await deleteVm(token, jobId);
      if (result.error) throw new Error(result.error);
      return result.success;
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: vmsKeys.list() });
    },
  });
}
