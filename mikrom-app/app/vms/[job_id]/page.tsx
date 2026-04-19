"use client";

import { useCallback, useEffect, useRef, useState, type ElementType, type ReactNode } from "react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { 
  HiChevronLeft, 
  HiRefresh, 
  HiClock, 
  HiServer, 
  HiHashtag, 
  HiExclamationCircle,
  HiTerminal,
  HiChip,
  HiStop,
  HiPause,
  HiPlay,
  HiTrash,
  HiLightningBolt
} from "react-icons/hi";
import { Loader2 } from "lucide-react";
import { 
  AreaChart, 
  Area, 
  XAxis, 
  YAxis, 
  CartesianGrid, 
  Tooltip, 
  ResponsiveContainer 
} from "recharts";
import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { getToken } from "@/lib/auth";
import { getVmLogsSSE, LogLine, pauseVm, resumeVm } from "@/lib/api";
import { useVm, useStopVm, useDeleteVm } from "@/lib/hooks/use-vms";
import Ansi from "ansi-to-react";
import { toast } from "sonner";

import { Badge, Alert, Progress } from "flowbite-react";
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
    <div className="flex items-start gap-4 py-4 border-b border-gray-100 dark:border-gray-800 last:border-0">
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

function MetricCard({ 
  icon: Icon, 
  label, 
  value, 
  percentage, 
  color 
}: { 
  icon: ElementType;
  label: string;
  value: string;
  percentage: number;
  color: string;
}) {
  return (
    <Card>
      <div className="flex items-center gap-2 text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
        <Icon className="w-4 h-4" /> {label}
      </div>
      <div className="text-2xl font-bold dark:text-white mb-3">{value}</div>
      <Progress progress={Math.min(100, Math.max(0, percentage))} color={color} size="sm" />
    </Card>
  );
}

interface MetricPoint {
  time: string;
  cpu: number;
  ram: number;
}

export default function VmDetailPage() {
  const params = useParams<{ job_id: string }>();
  const router = useRouter();
  const jobId = params.job_id as string;

  const { data: vm, isLoading, error, refetch, isFetching } = useVm(jobId);
  const stopVmMutation = useStopVm();
  const deleteVmMutation = useDeleteVm();

  const [confirmStop, setConfirmStop] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [metricsHistory, setMetricsHistory] = useState<MetricPoint[]>([]);
  const logEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (vm?.cpu_usage !== undefined) {
      setMetricsHistory(prev => {
        const newData: MetricPoint[] = [...prev, {
          time: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }),
          cpu: vm.cpu_usage * 100,
          ram: vm.ram_used_bytes / (1024 * 1024) // MiB
        }];
        return newData.slice(-20); // Keep last 20 points
      });
    }
  }, [vm]);

  const scrollToBottom = useCallback(() => {
    logEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, []);

  useEffect(() => {
    scrollToBottom();
  }, [logs, scrollToBottom]);

  const isStoppable = (status: string) => {
    const s = status.toLowerCase();
    return s === "running" || s === "scheduled" || s === "pending";
  };

  const [pausing, setPausing] = useState(false);
  const [resuming, setResuming] = useState(false);

  const handlePause = async () => {
    const token = getToken();
    if (!token) return;
    setPausing(true);
    try {
      await pauseVm(token, jobId);
      toast.success("Instance paused");
      refetch();
    } catch {
      toast.error("Failed to pause instance");
    } finally {
      setPausing(false);
    }
  };

  const handleResume = async () => {
    const token = getToken();
    if (!token) return;
    setResuming(true);
    try {
      await resumeVm(token, jobId);
      toast.success("Instance resumed");
      refetch();
    } catch {
      toast.error("Failed to resume instance");
    } finally {
      setResuming(false);
    }
  };

  const handleStop = async () => {
    setConfirmStop(false);
    toast.promise(stopVmMutation.mutateAsync(jobId), {
      loading: "Stopping instance...",
      success: "Instance stopped successfully",
      error: (err) => `Failed to stop: ${err.message}`,
    });
  };

  const handleDelete = async () => {
    setConfirmDelete(false);
    toast.promise(deleteVmMutation.mutateAsync(jobId), {
      loading: "Deleting instance...",
      success: () => {
        router.push("/vms");
        return "Instance deleted successfully";
      },
      error: (err) => `Failed to delete: ${err.message}`,
    });
  };

  useEffect(() => {
    let mounted = true;
    let closeLogs: (() => void) | null = null;

    if (mounted && jobId) {
      const token = getToken();
      if (token) {
        closeLogs = getVmLogsSSE(
          token,
          jobId,
          (log) => {
            if (mounted) setLogs((prev) => [...prev.slice(-499), log]);
          },
          (err) => console.error(err)
        );
      }
    }

    return () => {
      mounted = false;
      if (closeLogs) closeLogs();
    };
  }, [jobId]);

  const cpuPercent = vm ? vm.cpu_usage * 100 : 0;
  const ramMiB = vm ? vm.ram_used_bytes / (1024 * 1024) : 0;
  const ramPercent = (ramMiB / 512) * 100;

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="space-y-8">
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
            <div className="space-y-1">
              <Link href="/">
                <Button color="gray" size="xs" className="w-fit mb-2">
                  <HiChevronLeft className="w-4 h-4 mr-1" />
                  Back to Dashboard
                </Button>
              </Link>
              <h1 className="text-3xl font-bold text-zinc-900 dark:text-zinc-50 tracking-tight flex items-center gap-3">
                VM Detail
                {vm && (
                  <Badge color={getStatusColor(vm.status)} className="capitalize px-2 py-0.5 text-xs">
                    {normalizeStatus(vm.status)}
                  </Badge>
                )}
              </h1>
              <p className="text-zinc-500 dark:text-zinc-400 font-mono text-sm">
                ID: {jobId}
              </p>
            </div>
            <div className="flex items-center gap-3">
              <Button color="gray" size="sm" onClick={() => refetch()} disabled={isFetching}>
                <HiRefresh className={cn("w-4 h-4 mr-2", isFetching && "animate-spin")} />
                Refresh
              </Button>
              {vm && vm.status.toLowerCase() === "running" && (
                <Button color="gray" size="sm" onClick={handlePause} disabled={pausing}>
                  {pausing ? <Loader2 className="w-4 h-4 animate-spin" /> : <HiPause className="w-4 h-4 mr-2" />}
                  Pause
                </Button>
              )}
              {vm && vm.status.toLowerCase() === "paused" && (
                <Button color="gray" size="sm" onClick={handleResume} disabled={resuming}>
                  {resuming ? <Loader2 className="w-4 h-4 animate-spin" /> : <HiPlay className="w-4 h-4 mr-2" />}
                  Resume
                </Button>
              )}
              {vm && isStoppable(vm.status) && !confirmStop && (
                <Button variant="danger" size="sm" onClick={() => setConfirmStop(true)} disabled={stopVmMutation.isPending}>
                  <HiStop className="w-4 h-4 mr-2" />
                  Stop
                </Button>
              )}
              {confirmStop && (
                <div className="flex items-center gap-2 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg px-3 py-1.5">
                  <span className="text-xs font-medium text-red-700 dark:text-red-300">Stop?</span>
                  <Button variant="danger" size="xs" onClick={handleStop}>Confirm</Button>
                  <Button variant="ghost" size="xs" onClick={() => setConfirmStop(false)}>Cancel</Button>
                </div>
              )}
              {vm && !isStoppable(vm.status) && !confirmDelete && (
                <Button variant="danger" size="sm" onClick={() => setConfirmDelete(true)} disabled={deleteVmMutation.isPending}>
                  <HiTrash className="w-4 h-4 mr-2" />
                  Delete
                </Button>
              )}
              {confirmDelete && (
                <div className="flex items-center gap-2 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg px-3 py-1.5">
                  <span className="text-xs font-medium text-red-700 dark:text-red-300">Delete?</span>
                  <Button variant="danger" size="xs" onClick={handleDelete}>Confirm</Button>
                  <Button variant="ghost" size="xs" onClick={() => setConfirmDelete(false)}>Cancel</Button>
                </div>
              )}
            </div>
          </div>

          {error && (
            <Alert color="failure" icon={() => <HiExclamationCircle className="w-5 h-5 mr-2" />}>
              <div className="flex items-center justify-between w-full">
                <span>{error instanceof Error ? error.message : "Failed to load"}</span>
                <Button variant="outline" size="xs" onClick={() => refetch()}>Try again</Button>
              </div>
            </Alert>
          )}

          {vm && vm.status.toLowerCase() === "running" && (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mb-8">
              <Card>
                <div className="flex items-center gap-2 text-xs font-medium text-zinc-500 uppercase tracking-wider mb-4">
                  <HiChip className="w-4 h-4 text-blue-500" /> CPU Usage (%)
                </div>
                <div className="h-[200px] w-full">
                  <ResponsiveContainer width="100%" height="100%">
                    <AreaChart data={metricsHistory}>
                      <defs>
                        <linearGradient id="colorCpu" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%" stopColor="#3b82f6" stopOpacity={0.1}/>
                          <stop offset="95%" stopColor="#3b82f6" stopOpacity={0}/>
                        </linearGradient>
                      </defs>
                      <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="#e4e4e7" />
                      <XAxis dataKey="time" hide />
                      <YAxis hide domain={[0, 100]} />
                      <Tooltip 
                        contentStyle={{ backgroundColor: '#18181b', border: 'none', borderRadius: '8px', color: '#fff' }}
                        itemStyle={{ color: '#60a5fa' }}
                      />
                      <Area 
                        type="monotone" 
                        dataKey="cpu" 
                        stroke="#3b82f6" 
                        fillOpacity={1} 
                        fill="url(#colorCpu)" 
                        strokeWidth={2}
                        isAnimationActive={false}
                      />
                    </AreaChart>
                  </ResponsiveContainer>
                </div>
                <div className="mt-4 flex items-center justify-between">
                  <span className="text-2xl font-bold dark:text-white">{(vm.cpu_usage * 100).toFixed(1)}%</span>
                  <Badge color="info">Real-time</Badge>
                </div>
              </Card>

              <Card>
                <div className="flex items-center gap-2 text-xs font-medium text-zinc-500 uppercase tracking-wider mb-4">
                  <HiLightningBolt className="w-4 h-4 text-amber-500" /> Memory Usage (MiB)
                </div>
                <div className="h-[200px] w-full">
                  <ResponsiveContainer width="100%" height="100%">
                    <AreaChart data={metricsHistory}>
                      <defs>
                        <linearGradient id="colorRam" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%" stopColor="#f59e0b" stopOpacity={0.1}/>
                          <stop offset="95%" stopColor="#f59e0b" stopOpacity={0}/>
                        </linearGradient>
                      </defs>
                      <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="#e4e4e7" />
                      <XAxis dataKey="time" hide />
                      <YAxis hide />
                      <Tooltip 
                        contentStyle={{ backgroundColor: '#18181b', border: 'none', borderRadius: '8px', color: '#fff' }}
                        itemStyle={{ color: '#fbbf24' }}
                      />
                      <Area 
                        type="monotone" 
                        dataKey="ram" 
                        stroke="#f59e0b" 
                        fillOpacity={1} 
                        fill="url(#colorRam)" 
                        strokeWidth={2}
                        isAnimationActive={false}
                      />
                    </AreaChart>
                  </ResponsiveContainer>
                </div>
                <div className="mt-4 flex items-center justify-between">
                  <span className="text-2xl font-bold dark:text-white">{(vm.ram_used_bytes / (1024 * 1024)).toFixed(1)} MiB</span>
                  <Badge color="warning">Live</Badge>
                </div>
              </Card>
            </div>
          )}

          {isLoading ? (
            <div className="flex flex-col items-center justify-center py-32 text-zinc-500">
              <Loader2 className="w-10 h-10 animate-spin mb-4 opacity-20" />
              <p className="text-sm font-medium">Retrieving instance data...</p>
            </div>
          ) : vm ? (
            <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
              <div className="lg:col-span-2 space-y-8">
                {/* Visual Metrics */}
                <div className="grid grid-cols-2 gap-4">
                  <MetricCard 
                    icon={HiChip} 
                    label="CPU Usage" 
                    value={`${cpuPercent.toFixed(1)}%`}
                    percentage={cpuPercent}
                    color={cpuPercent > 80 ? "failure" : "info"}
                  />
                  <MetricCard 
                    icon={HiServer} 
                    label="RAM Used" 
                    value={`${ramMiB.toFixed(1)} MB`}
                    percentage={ramPercent}
                    color={ramPercent > 80 ? "failure" : "success"}
                  />
                </div>

                {/* Logs */}
                <Card noPadding>
                  <div className="p-4 border-b border-gray-100 dark:border-gray-800 bg-white dark:bg-gray-800 rounded-t-xl">
                    <h5 className="text-sm font-bold flex items-center gap-2 dark:text-white">
                      <HiTerminal className="w-4 h-4 text-gray-400" />
                      Live Console Logs
                    </h5>
                  </div>
                  <div className="h-[400px] bg-zinc-950 overflow-y-auto font-mono text-[10px] sm:text-xs text-zinc-300 p-4">
                    <div className="space-y-1">
                      {logs.length === 0 ? (
                        <div className="text-zinc-600 italic">Waiting for console output...</div>
                      ) : (
                        logs.map((log, i) => (
                          <div key={i} className="flex gap-4">
                            <span className="text-zinc-600 shrink-0 select-none">
                              {new Date(log.timestamp * 1000).toLocaleTimeString([], { hour12: false })}
                            </span>
                            <span className="whitespace-pre-wrap break-all">
                              <Ansi useClasses>{log.line}</Ansi>
                            </span>
                          </div>
                        ))
                      )}
                      <div ref={logEndRef} />
                    </div>
                  </div>
                </Card>

                <Card>
                  <h5 className="text-sm font-bold flex items-center gap-2 dark:text-white mb-4">
                    <HiTerminal className="w-4 h-4 text-gray-400" />
                    Instance Configuration
                  </h5>
                  <div className="space-y-1">
                    <DetailRow icon={HiHashtag} label="Job ID" value={vm.job_id} mono />
                    <DetailRow icon={HiServer} label="Host Identifier" value={vm.host_id || "Not assigned yet"} mono={!!vm.host_id} />
                    <DetailRow icon={HiChip} label="VM Internal ID" value={vm.vm_id || "Not created yet"} mono={!!vm.vm_id} />
                  </div>
                </Card>
              </div>

              <div className="space-y-8">
                <Card>
                  <h5 className="text-sm font-bold flex items-center gap-2 dark:text-white mb-6">
                    <HiClock className="w-4 h-4 text-gray-400" />
                    Lifecycle Events
                  </h5>
                  <div className="space-y-6 relative">
                    <div className="absolute left-[7px] top-2 bottom-2 w-px bg-gray-200 dark:bg-gray-700" />
                    <div className="relative flex items-center gap-4">
                      <div className="w-4 h-4 rounded-full bg-gray-200 dark:bg-gray-700 z-10 border-2 border-white dark:border-gray-800" />
                      <div>
                        <p className="text-xs font-bold text-gray-400 uppercase tracking-tighter">Scheduled</p>
                        <p className="text-xs text-gray-600 dark:text-gray-400">{formatTimestamp(vm.scheduled_at)}</p>
                      </div>
                    </div>
                    <div className="relative flex items-center gap-4">
                      <div className={cn("w-4 h-4 rounded-full z-10 border-2 border-white dark:border-gray-800", vm.started_at ? "bg-green-500" : "bg-gray-200 dark:bg-gray-700")} />
                      <div>
                        <p className="text-xs font-bold text-gray-400 uppercase tracking-tighter">Started</p>
                        <p className="text-xs text-gray-600 dark:text-gray-400">{formatTimestamp(vm.started_at)}</p>
                      </div>
                    </div>
                    <div className="relative flex items-center gap-4">
                      <div className={cn("w-4 h-4 rounded-full z-10 border-2 border-white dark:border-gray-800", (vm.stopped_at || !isStoppable(vm.status)) ? "bg-red-500" : "bg-gray-200 dark:bg-gray-700")} />
                      <div>
                        <p className="text-xs font-bold text-gray-400 uppercase tracking-tighter">Stopped</p>
                        <p className="text-xs text-gray-600 dark:text-gray-400">{formatTimestamp(vm.stopped_at)}</p>
                      </div>
                    </div>
                  </div>
                </Card>
              </div>
            </div>
          ) : null}
        </div>
      </DashboardLayout>
    </AuthGuard>
  );
}
