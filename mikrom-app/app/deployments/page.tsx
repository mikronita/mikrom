"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { 
  HiRefresh, 
  HiServer, 
  HiSearch,
  HiExternalLink,
  HiFilter,
  HiStop,
  HiTrash,
  HiExclamationCircle
} from "react-icons/hi";
import { Loader2 } from "lucide-react";
import Link from "next/link";

import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { getToken } from "@/lib/auth";
import { listVms, stopVm, deleteVm, VmInfo } from "@/lib/api";

import { Badge, Table, TableBody, TableCell, TableHead, TableHeadCell, TableRow, TextInput, Alert, Button, Card } from "flowbite-react";
import { cn } from "@/lib/utils";

function normalizeStatus(status: string): string {
  return status.toLowerCase() === "cancelled" ? "stopped" : status;
}

function getStatusColor(status: string): string {
  const s = status.toLowerCase();
  if (s === "running") return "success";
  if (s === "scheduled" || s === "pending") return "warning";
  if (s === "failed" || s === "cancelled") return "failure";
  return "gray";
}

export default function DeploymentsPage() {
  const [deployments, setDeployments] = useState<VmInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState("");
  const [stoppingId, setStoppingId] = useState<string | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [confirmStopId, setConfirmStopId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [stopError, setStopError] = useState<string | null>(null);

  const fetchDeployments = useCallback(async (isInitial = true) => {
    const token = getToken();
    if (!token) return;
    if (isInitial) setLoading(true);
    const result = await listVms(token);
    if (!result.error) {
      setDeployments(result.data ?? []);
    }
    if (isInitial) setLoading(false);
  }, []);

  const handleStop = async (jobId: string) => {
    const token = getToken();
    if (!token) return;
    setStoppingId(jobId);
    setConfirmStopId(null);
    setStopError(null);
    const result = await stopVm(token, jobId);
    setStoppingId(null);
    if (result.error) {
      setStopError(result.error);
    } else {
      await fetchDeployments(false);
    }
  };

  const handleDelete = async (jobId: string) => {
    const token = getToken();
    if (!token) return;
    setDeletingId(jobId);
    setConfirmDeleteId(null);
    setStopError(null);
    const result = await deleteVm(token, jobId);
    setDeletingId(null);
    if (result.error) {
      setStopError(result.error);
    } else {
      await fetchDeployments(false);
    }
  };

  const isStoppable = (status: string) => {
    const s = status.toLowerCase();
    return s === "running" || s === "scheduled" || s === "pending";
  };

  const deploymentsRef = useRef(deployments);
  useEffect(() => {
    deploymentsRef.current = deployments;
  }, [deployments]);

  useEffect(() => {
    let isMounted = true;
    
    const init = async () => {
      const token = getToken();
      if (!token) return;
      const result = await listVms(token);
      if (isMounted && !result.error) {
        setDeployments(result.data ?? []);
        setLoading(false);
      }
    };
    
    init();

    const intervalId = setInterval(async () => {
      const hasTransitional = deploymentsRef.current.some(vm => {
        const s = vm.status.toLowerCase();
        return s === "pending" || s === "scheduled" || s === "stopping";
      });

      if (hasTransitional) {
        const token = getToken();
        if (token) {
          const result = await listVms(token);
          if (isMounted && !result.error) {
            setDeployments(result.data ?? []);
          }
        }
      }
    }, 3000);

    return () => {
      isMounted = false;
      clearInterval(intervalId);
    };
  }, []);

  const filteredDeployments = deployments.filter(vm =>
    vm.app_name.toLowerCase().includes(searchQuery.toLowerCase()) ||
    vm.image.toLowerCase().includes(searchQuery.toLowerCase())
  );

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="space-y-6">
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
            <div>
              <h1 className="text-2xl font-bold text-zinc-900 dark:text-zinc-50 tracking-tight">
                Active Deployments
              </h1>
              <p className="text-zinc-500 dark:text-zinc-400 text-sm mt-1">
                Monitor and manage your running application instances.
              </p>
            </div>
            <div className="flex items-center gap-3">
              <Button color="gray" size="sm" onClick={() => fetchDeployments(true)} disabled={loading}>
                <HiRefresh className={cn("w-4 h-4 mr-2", loading && "animate-spin")} />
                Refresh
              </Button>
            </div>
          </div>

          <Card className="overflow-hidden border-none shadow-sm ring-1 ring-zinc-200 dark:ring-zinc-800">
            <div className="p-4 border-b border-zinc-100 dark:border-zinc-800 flex flex-col sm:flex-row sm:items-center justify-between gap-4">
              <div className="relative w-full sm:max-w-xs">
                <div className="absolute inset-y-0 left-0 flex items-center pl-3 pointer-events-none">
                  <HiSearch className="w-4 h-4 text-zinc-400" />
                </div>
                <TextInput
                  placeholder="Search deployments..."
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="pl-10"
                  sizing="sm"
                />
              </div>
              <div className="flex items-center gap-2">
                <Button color="gray" size="xs">
                  <HiFilter className="w-3 h-3 mr-2" />
                  Filter
                </Button>
              </div>
            </div>

            {stopError && (
              <Alert color="failure" className="rounded-none" icon={() => <HiExclamationCircle className="w-4 h-4 mr-2" />}>
                {stopError}
              </Alert>
            )}

            <div className="overflow-x-auto">
              <Table hoverable>
                <TableHead>
                  <TableRow>
                    <TableHeadCell>Application</TableHeadCell>
                    <TableHeadCell>Status</TableHeadCell>
                    <TableHeadCell>Image</TableHeadCell>
                    <TableHeadCell>Resources</TableHeadCell>
                    <TableHeadCell className="text-right">Actions</TableHeadCell>
                  </TableRow>
                </TableHead>
                <TableBody className="divide-y">
                  {loading && deployments.length === 0 ? (
                    Array.from({ length: 5 }).map((_, i) => (
                      <TableRow key={i} className="animate-pulse">
                        <TableCell colSpan={5} className="py-6">
                          <div className="h-4 bg-zinc-100 dark:bg-zinc-800 rounded w-full" />
                        </TableCell>
                      </TableRow>
                    ))
                  ) : filteredDeployments.length === 0 ? (
                    <TableRow>
                      <TableCell colSpan={5} className="text-center py-12 text-zinc-500">
                        No deployments found.
                      </TableCell>
                    </TableRow>
                  ) : (
                    filteredDeployments.map((vm) => (
                      <TableRow key={vm.job_id} className="bg-white dark:border-zinc-700 dark:bg-zinc-900">
                        <TableCell>
                          <div className="flex items-center gap-3">
                            <div className="w-8 h-8 rounded-lg flex items-center justify-center bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700">
                              <HiServer className="w-4 h-4 text-zinc-400" />
                            </div>
                            <div className="font-medium text-zinc-900 dark:text-white">
                              {vm.app_name}
                            </div>
                          </div>
                        </TableCell>
                        <TableCell>
                          <Badge color={getStatusColor(vm.status)} className="capitalize w-fit">
                            {normalizeStatus(vm.status)}
                          </Badge>
                        </TableCell>
                        <TableCell className="font-mono text-xs text-zinc-500 dark:text-zinc-400">
                          {vm.image}
                        </TableCell>
                        <TableCell className="text-xs text-zinc-500 dark:text-zinc-400">
                          {vm.vcpus} vCPU • {vm.memory_mib} MiB
                        </TableCell>
                        <TableCell>
                          <div className="flex items-center justify-end gap-2">
                            {isStoppable(vm.status) && (
                              <Button
                                color={confirmStopId === vm.job_id ? "warning" : "gray"}
                                size="xs"
                                onClick={() => confirmStopId === vm.job_id ? handleStop(vm.job_id) : setConfirmStopId(vm.job_id)}
                                disabled={!!stoppingId}
                              >
                                {stoppingId === vm.job_id ? (
                                  <Loader2 className="w-3 h-3 animate-spin mr-1" />
                                ) : (
                                  <HiStop className="w-3 h-3 mr-1" />
                                )}
                                {confirmStopId === vm.job_id ? "Confirm?" : "Stop"}
                              </Button>
                            )}
                            {!isStoppable(vm.status) && (
                              <Button
                                color={confirmDeleteId === vm.job_id ? "failure" : "gray"}
                                size="xs"
                                onClick={() => confirmDeleteId === vm.job_id ? handleDelete(vm.job_id) : setConfirmDeleteId(vm.job_id)}
                                disabled={!!deletingId}
                              >
                                {deletingId === vm.job_id ? (
                                  <Loader2 className="w-3 h-3 animate-spin mr-1" />
                                ) : (
                                  <HiTrash className="w-3 h-3 mr-1" />
                                )}
                                {confirmDeleteId === vm.job_id ? "Confirm?" : "Delete"}
                              </Button>
                            )}
                            <Link href={`/deployments/${vm.job_id}`}>
                              <Button color="gray" size="xs">
                                Details
                                <HiExternalLink className="w-3 h-3 ml-1" />
                              </Button>
                            </Link>
                          </div>
                        </TableCell>
                      </TableRow>
                    ))
                  )}
                </TableBody>
              </Table>
            </div>
          </Card>
        </div>
      </DashboardLayout>
    </AuthGuard>
  );
}
