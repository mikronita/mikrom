"use client";

import { useCallback, useEffect, useRef, useState, type ElementType, type ReactNode } from "react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { 
  ChevronLeft, 
  RefreshCw, 
  Clock, 
  Server, 
  Hash, 
  AlertCircle,
  Loader2,
  Terminal,
  Cpu,
  Square,
  Trash2
} from "lucide-react";
import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { getToken } from "@/lib/auth";
import { getVm, stopVm, deleteVm, VmStatus } from "@/lib/api";

import { Button } from "@/components/ui/Button";
import { Badge } from "@/components/ui/Badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/Card";
import { cn } from "@/lib/utils";

function normalizeStatus(status: string): string {
  return status.toLowerCase() === "cancelled" ? "stopped" : status;
}

function getStatusVariant(status: string): "success" | "warning" | "danger" | "secondary" {
  const s = status.toLowerCase();
  if (s === "running") return "success";
  if (s === "scheduled" || s === "pending") return "warning";
  if (s === "failed" || s === "cancelled") return "danger";
  return "secondary";
}

function formatTimestamp(ts: number): string {
  if (!ts) return "—";
  return new Date(ts * 1000).toLocaleString();
}

function DetailRow({ 
  icon: Icon, 
  label, 
  value, 
  mono = false 
}: { 
  icon: ElementType;
  label: string; 
  value: ReactNode;
  mono?: boolean;
}) {
  return (
    <div className="flex items-start gap-4 py-4">
      <div className="mt-0.5 w-8 h-8 rounded-lg bg-zinc-100 dark:bg-zinc-800 flex items-center justify-center shrink-0">
        <Icon className="w-4 h-4 text-zinc-500" />
      </div>
      <div className="flex-1 min-w-0">
        <dt className="text-xs font-medium text-zinc-500 dark:text-zinc-400 uppercase tracking-wider mb-1">
          {label}
        </dt>
        <dd className={cn(
          "text-sm text-zinc-900 dark:text-zinc-100 break-all",
          mono && "font-mono bg-zinc-50 dark:bg-zinc-800/50 px-1.5 py-0.5 rounded"
        )}>
          {value}
        </dd>
      </div>
    </div>
  );
}

export default function VmDetailPage() {
  const params = useParams<{ job_id: string }>();
  const jobId = params.job_id;

  const [vm, setVm] = useState<VmStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [stopping, setStopping] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [stopError, setStopError] = useState<string | null>(null);
  const [confirmStop, setConfirmStop] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const vmRef = useRef<VmStatus | null>(null);
  useEffect(() => { vmRef.current = vm; }, [vm]);

  const fetchVm = useCallback(async (silent = false) => {
    const token = getToken();
    if (!token) return;
    if (!silent) setLoading(true);
    setError(null);
    const result = await getVm(token, jobId);
    if (result.error) {
      setError(result.error);
    } else {
      setVm(result.data ?? null);
    }
    if (!silent) setLoading(false);
  }, [jobId]);

  const isStoppable = (status: string) => {
    const s = status.toLowerCase();
    return s === "running" || s === "scheduled" || s === "pending";
  };

  const handleStop = async () => {
    const token = getToken();
    if (!token) return;
    setStopping(true);
    setStopError(null);
    setConfirmStop(false);
    const result = await stopVm(token, jobId);
    setStopping(false);
    if (result.error) {
      setStopError(result.error);
    } else {
      await fetchVm();
    }
  };

  const handleDelete = async () => {
    const token = getToken();
    if (!token) return;
    setDeleting(true);
    setStopError(null);
    setConfirmDelete(false);
    const result = await deleteVm(token, jobId);
    setDeleting(false);
    if (result.error) {
      setStopError(result.error);
    } else {
      window.location.href = "/dashboard/vms";
    }
  };

  useEffect(() => {
    let mounted = true;

    const init = async () => {
      if (mounted) {
        await fetchVm();
      }
    };

    init();

    const interval = setInterval(() => {
      if (mounted) {
        const current = vmRef.current;
        if (!current || isStoppable(current.status)) {
          fetchVm(true);
        }
      }
    }, 5000);

    return () => {
      mounted = false;
      clearInterval(interval);
    };
  }, [fetchVm]);

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="p-8 max-w-4xl mx-auto space-y-8">
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
            <div className="space-y-1">
              <Link href="/dashboard">
                <Button variant="ghost" size="sm" className="-ml-2 h-8 text-zinc-500 hover:text-zinc-900">
                  <ChevronLeft className="w-4 h-4 mr-1" />
                  Back to Dashboard
                </Button>
              </Link>
              <h1 className="text-3xl font-bold text-zinc-900 dark:text-zinc-50 tracking-tight flex items-center gap-3">
                VM Detail
                {vm && (
                  <Badge variant={getStatusVariant(vm.status)} className="capitalize px-2 py-0.5 text-xs">
                    {normalizeStatus(vm.status)}
                  </Badge>
                )}
              </h1>
              <p className="text-zinc-500 dark:text-zinc-400 font-mono text-sm">
                ID: {jobId}
              </p>
            </div>
            <div className="flex items-center gap-3">
              <Button variant="outline" size="sm" onClick={() => fetchVm()} disabled={loading}>
                <RefreshCw className={cn("w-4 h-4 mr-2", loading && "animate-spin")} />
                Refresh
              </Button>
              {vm && isStoppable(vm.status) && !confirmStop && (
                <Button
                  size="sm"
                  variant="danger"
                  onClick={() => setConfirmStop(true)}
                  disabled={stopping}
                >
                  {stopping ? (
                    <><Loader2 className="w-4 h-4 mr-2 animate-spin" />Stopping...</>
                  ) : (
                    <><Square className="w-4 h-4 mr-2" />Stop Instance</>
                  )}
                </Button>
              )}
              {confirmStop && (
                <div className="flex items-center gap-2 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg px-3 py-1.5">
                  <span className="text-xs font-medium text-red-700 dark:text-red-300">Stop this instance?</span>
                  <Button size="sm" variant="danger" className="h-6 px-2 text-xs" onClick={handleStop}>
                    Confirm
                  </Button>
                  <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" onClick={() => setConfirmStop(false)}>
                    Cancel
                  </Button>
                </div>
              )}
              {vm && !isStoppable(vm.status) && !confirmDelete && (
                <Button
                  size="sm"
                  variant="danger"
                  onClick={() => setConfirmDelete(true)}
                  disabled={deleting}
                >
                  {deleting ? (
                    <><Loader2 className="w-4 h-4 mr-2 animate-spin" />Deleting...</>
                  ) : (
                    <><Trash2 className="w-4 h-4 mr-2" />Delete Instance</>
                  )}
                </Button>
              )}
              {confirmDelete && (
                <div className="flex items-center gap-2 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg px-3 py-1.5">
                  <span className="text-xs font-medium text-red-700 dark:text-red-300">Delete this instance?</span>
                  <Button size="sm" variant="danger" className="h-6 px-2 text-xs" onClick={handleDelete}>
                    Confirm
                  </Button>
                  <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" onClick={() => setConfirmDelete(false)}>
                    Cancel
                  </Button>
                </div>
              )}
            </div>
          </div>

          {error && (
            <div className="p-4 flex items-center gap-3 text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/10 border border-red-100 dark:border-red-900/20 rounded-xl">
              <AlertCircle className="w-5 h-5" />
              <div className="flex-1 font-medium">{error}</div>
              <Button size="sm" variant="outline" onClick={() => fetchVm()} className="h-8">Try again</Button>
            </div>
          )}

          {stopError && (
            <div className="p-4 flex items-center gap-3 text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/10 border border-red-100 dark:border-red-900/20 rounded-xl">
              <AlertCircle className="w-5 h-5" />
              <div className="flex-1 font-medium">Stop failed: {stopError}</div>
              <Button size="sm" variant="ghost" className="h-8" onClick={() => setStopError(null)}>Dismiss</Button>
            </div>
          )}

          {loading && !vm ? (
            <div className="flex flex-col items-center justify-center py-32 text-zinc-500">
              <Loader2 className="w-10 h-10 animate-spin mb-4 opacity-20" />
              <p className="text-sm font-medium tracking-wide">Retrieving instance data...</p>
            </div>
          ) : vm ? (
            <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
              <div className="md:col-span-2 space-y-8">
                <Card>
                  <CardHeader className="border-b border-zinc-100 dark:border-zinc-800">
                    <CardTitle className="text-base flex items-center gap-2">
                      <Terminal className="w-4 h-4 text-zinc-400" />
                      Instance Configuration
                    </CardTitle>
                  </CardHeader>
                  <CardContent className="divide-y divide-zinc-100 dark:divide-zinc-800">
                    <DetailRow 
                      icon={Hash} 
                      label="Job ID" 
                      value={vm.job_id} 
                      mono 
                    />
                    <DetailRow 
                      icon={Server} 
                      label="Host Identifier" 
                      value={vm.host_id || "Not assigned yet"} 
                      mono={!!vm.host_id}
                    />
                    <DetailRow 
                      icon={Cpu} 
                      label="VM Internal ID" 
                      value={vm.vm_id || "Not created yet"} 
                      mono={!!vm.vm_id}
                    />
                  </CardContent>
                </Card>

                {vm.error_message && (
                  <Card className="border-red-200 dark:border-red-900/30 bg-red-50/30 dark:bg-red-900/5">
                    <CardHeader>
                      <CardTitle className="text-red-600 dark:text-red-400 flex items-center gap-2">
                        <AlertCircle className="w-4 h-4" />
                        Deployment Error
                      </CardTitle>
                    </CardHeader>
                    <CardContent>
                      <div className="p-4 rounded-lg bg-white dark:bg-zinc-900 border border-red-100 dark:border-red-900/20 text-sm font-mono text-red-600 dark:text-red-400">
                        {vm.error_message}
                      </div>
                    </CardContent>
                  </Card>
                )}
              </div>

              <div className="space-y-8">
                <Card>
                  <CardHeader className="border-b border-zinc-100 dark:border-zinc-800">
                    <CardTitle className="text-base flex items-center gap-2">
                      <Clock className="w-4 h-4 text-zinc-400" />
                      Lifecycle Events
                    </CardTitle>
                  </CardHeader>
                  <CardContent className="p-0">
                    <div className="px-6 py-4 space-y-6 relative">
                      {/* Timeline Line */}
                      <div className="absolute left-[31px] top-8 bottom-8 w-px bg-zinc-200 dark:bg-zinc-800" />
                      
                      <div className="relative flex items-center gap-4">
                        <div className="w-4 h-4 rounded-full bg-zinc-200 dark:bg-zinc-800 z-10" />
                        <div>
                          <p className="text-xs font-bold text-zinc-400 uppercase tracking-tighter">Scheduled</p>
                          <p className="text-xs text-zinc-600 dark:text-zinc-400">{formatTimestamp(vm.scheduled_at)}</p>
                        </div>
                      </div>

                      <div className="relative flex items-center gap-4">
                        <div className={cn(
                          "w-4 h-4 rounded-full z-10",
                          vm.started_at ? "bg-green-500" : "bg-zinc-200 dark:bg-zinc-800"
                        )} />
                        <div>
                          <p className="text-xs font-bold text-zinc-400 uppercase tracking-tighter">Started</p>
                          <p className="text-xs text-zinc-600 dark:text-zinc-400">{formatTimestamp(vm.started_at)}</p>
                        </div>
                      </div>

                      <div className="relative flex items-center gap-4">
                        <div className={cn(
                          "w-4 h-4 rounded-full z-10",
                          (vm.stopped_at || !isStoppable(vm.status)) ? "bg-red-500" : "bg-zinc-200 dark:bg-zinc-800"
                        )} />
                        <div>
                          <p className="text-xs font-bold text-zinc-400 uppercase tracking-tighter">Stopped</p>
                          <p className="text-xs text-zinc-600 dark:text-zinc-400">{formatTimestamp(vm.stopped_at)}</p>
                        </div>
                      </div>
                    </div>
                  </CardContent>
                </Card>
              </div>
            </div>
          ) : null}
        </div>
      </DashboardLayout>
    </AuthGuard>
  );
}
