"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { 
  HiPlus, 
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

import { Badge, Table, TableBody, TableCell, TableHead, TableHeadCell, TableRow, TextInput, Alert } from "flowbite-react";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
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

export default function VmsPage() {
  const [vms, setVms] = useState<VmInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [stoppingId, setStoppingId] = useState<string | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [confirmStopId, setConfirmStopId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [stopError, setStopError] = useState<string | null>(null);

  const fetchVms = useCallback(async () => {
    const token = getToken();
    if (!token) return;
    setLoading(true);
    setError(null);
    const result = await listVms(token);
    if (result.error) {
      setError(result.error);
    } else {
      setVms(result.data ?? []);
    }
    setLoading(false);
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
      await fetchVms();
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
      await fetchVms();
    }
  };

  const isStoppable = (status: string) => {
    const s = status.toLowerCase();
    return s === "running" || s === "scheduled" || s === "pending";
  };

  const vmsRef = useRef(vms);
  useEffect(() => {
    vmsRef.current = vms;
  }, [vms]);

  useEffect(() => {
    const init = async () => {
      await fetchVms();
    };
    init();

    const intervalId = setInterval(async () => {
      const hasTransitional = vmsRef.current.some(vm => {
        const s = vm.status.toLowerCase();
        return s === "pending" || s === "scheduled" || s === "starting" || s === "stopping";
      });

      if (hasTransitional) {
        const token = getToken();
        if (token) {
          const result = await listVms(token);
          if (!result.error && result.data) {
            setVms(result.data);
          }
        }
      }
    }, 5000);

    return () => clearInterval(intervalId);
  }, [fetchVms]);

  const filteredVms = vms.filter(vm => 
    vm.app_name.toLowerCase().includes(searchQuery.toLowerCase()) ||
    vm.image.toLowerCase().includes(searchQuery.toLowerCase()) ||
    vm.job_id.toLowerCase().includes(searchQuery.toLowerCase())
  );

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="space-y-6">
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
            <div>
              <h1 className="text-3xl font-bold text-zinc-900 dark:text-zinc-50 tracking-tight">
                Virtual Machines
              </h1>
              <p className="text-zinc-500 dark:text-zinc-400 mt-1">
                Manage and monitor all your running instances.
              </p>
            </div>
            <div className="flex items-center gap-3">
              <Button color="gray" size="sm" onClick={fetchVms} disabled={loading}>
                <HiRefresh className={cn("w-4 h-4 mr-2", loading && "animate-spin")} />
                Refresh
              </Button>
              <Button as={Link} href="/" size="sm" color="dark">
                <HiPlus className="w-4 h-4 mr-2" />
                New Instance
              </Button>
            </div>
          </div>

          {error && (
            <Alert color="failure" icon={() => <HiExclamationCircle className="w-4 h-4 mr-2" />}>
              {error}
            </Alert>
          )}

          {stopError && (
            <Alert color="failure" onDismiss={() => setStopError(null)} icon={() => <HiExclamationCircle className="w-4 h-4 mr-2" />}>
              Action failed: {stopError}
            </Alert>
          )}

          <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
            <div className="w-full md:w-96">
              <TextInput 
                icon={HiSearch}
                placeholder="Filter by name, image or ID..." 
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
              />
            </div>
            <Button color="gray" size="sm">
              <HiFilter className="w-4 h-4 mr-2" />
              Filters
            </Button>
          </div>

          <Card noPadding>
            <div className="overflow-x-auto">
              <Table hoverable>
                <TableHead>
                  <TableRow>
                    <TableHeadCell>Application</TableHeadCell>
                    <TableHeadCell>Status</TableHeadCell>
                    <TableHeadCell>Image</TableHeadCell>
                    <TableHeadCell>Job ID</TableHeadCell>
                    <TableHeadCell>
                      <span className="sr-only">Actions</span>
                    </TableHeadCell>
                  </TableRow>
                </TableHead>
                <TableBody className="divide-y">
                  {loading && vms.length === 0 ? (
                    Array.from({ length: 5 }).map((_, i) => (
                      <TableRow key={i} className="bg-white dark:border-gray-700 dark:bg-gray-800">
                        <TableCell className="whitespace-nowrap font-medium text-gray-900 dark:text-white">
                          <div className="h-4 w-32 bg-gray-200 dark:bg-gray-700 animate-pulse rounded" />
                        </TableCell>
                        <TableCell>
                          <div className="h-4 w-16 bg-gray-200 dark:bg-gray-700 animate-pulse rounded-full" />
                        </TableCell>
                        <TableCell>
                          <div className="h-4 w-40 bg-gray-200 dark:bg-gray-700 animate-pulse rounded" />
                        </TableCell>
                        <TableCell>
                          <div className="h-4 w-24 bg-gray-200 dark:bg-gray-700 animate-pulse rounded" />
                        </TableCell>
                        <TableCell>
                          <div className="h-8 w-20 bg-gray-200 dark:bg-gray-700 animate-pulse rounded" />
                        </TableCell>
                      </TableRow>
                    ))
                  ) : filteredVms.length === 0 ? (
                    <TableRow className="bg-white dark:border-gray-700 dark:bg-gray-800">
                      <TableCell colSpan={5} className="text-center py-20">
                        <HiServer className="w-10 h-10 mx-auto text-gray-400 mb-4" />
                        <p className="text-gray-500">No instances found</p>
                      </TableCell>
                    </TableRow>
                  ) : (
                    filteredVms.map((vm) => (
                      <TableRow key={vm.job_id} className="bg-white dark:border-gray-700 dark:bg-gray-800">
                        <TableCell className="whitespace-nowrap font-bold text-gray-900 dark:text-white">
                          {vm.app_name}
                        </TableCell>
                        <TableCell>
                          <Badge color={getStatusColor(vm.status)} className="w-fit">
                            {normalizeStatus(vm.status)}
                          </Badge>
                        </TableCell>
                        <TableCell className="font-mono text-xs">
                          {vm.image}
                        </TableCell>
                        <TableCell className="text-xs text-gray-500">
                          {vm.job_id.slice(0, 8)}...
                        </TableCell>
                        <TableCell>
                          <div className="flex items-center gap-2 px-4 py-2">
                             {isStoppable(vm.status) && (
                              <Button
                                color={confirmStopId === vm.job_id ? "failure" : "gray"}
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
                            <Button as={Link} href={`/vms/${vm.job_id}`} color="gray" size="xs">
                              Details
                              <HiExternalLink className="w-3 h-3 ml-1" />
                            </Button>
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
