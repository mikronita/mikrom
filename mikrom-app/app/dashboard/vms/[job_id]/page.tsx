"use client";

import { useCallback, useEffect, useRef, useState, type ElementType, type ReactNode } from "react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
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
  Pause,
  Play,
  Trash2,
  Zap
} from "lucide-react";
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

function MetricCard({ 
  icon: Icon, 
  label, 
  value, 
  percentage, 
  colorClass 
}: { 
  icon: ElementType;
  label: string;
  value: string;
  percentage: number;
  colorClass: string;
}) {
  return (
    <Card>
      <CardHeader className="py-4">
        <CardTitle className="text-xs font-medium text-zinc-500 flex items-center gap-2 uppercase tracking-wider">
          <Icon className="w-4 h-4" /> {label}
        </CardTitle>
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-bold">{value}</div>
        <div className="mt-3 h-1.5 w-full bg-zinc-100 dark:bg-zinc-800 rounded-full overflow-hidden">
          <div 
            className={cn("h-full transition-all duration-500 rounded-full", colorClass)}
            style={{ width: `${Math.min(100, Math.max(0, percentage))}%` }}
          />
        </div>
      </CardContent>
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
  const jobId = params.job_id;

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
        router.push("/dashboard/vms");
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
  // Asumiendo un límite de RAM de 512MB para el porcentaje si no se conoce el límite
  const ramMiB = vm ? vm.ram_used_bytes / (1024 * 1024) : 0;
  const ramPercent = (ramMiB / 512) * 100;

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
              <Button variant="outline" size="sm" onClick={() => refetch()} disabled={isFetching}>
                <RefreshCw className={cn("w-4 h-4 mr-2", isFetching && "animate-spin")} />
                Refresh
              </Button>
              {vm && vm.status.toLowerCase() === "running" && (
                <Button
                  size="sm"
                  variant="outline"
                  onClick={handlePause}
                  disabled={pausing}
                >
                  {pausing ? <Loader2 className="w-4 h-4 animate-spin" /> : <Pause className="w-4 h-4 mr-2" />}
                  Pause
                </Button>
              )}
              {vm && vm.status.toLowerCase() === "paused" && (
                <Button
                  size="sm"
                  variant="outline"
                  onClick={handleResume}
                  disabled={resuming}
                >
                  {resuming ? <Loader2 className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4 mr-2" />}
                  Resume
                </Button>
              )}
              {vm && isStoppable(vm.status) && !confirmStop && (
                <Button
                  size="sm"
                  variant="danger"
                  onClick={() => setConfirmStop(true)}
                  disabled={stopVmMutation.isPending}
                >
                  <Square className="w-4 h-4 mr-2" />
                  Stop
                </Button>
              )}
              {confirmStop && (
                <div className="flex items-center gap-2 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg px-3 py-1.5">
                  <span className="text-xs font-medium text-red-700 dark:text-red-300">Stop?</span>
                  <Button size="sm" variant="danger" className="h-6 px-2 text-xs" onClick={handleStop}>Confirm</Button>
                  <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" onClick={() => setConfirmStop(false)}>Cancel</Button>
                </div>
              )}
              {vm && !isStoppable(vm.status) && !confirmDelete && (
                <Button
                  size="sm"
                  variant="danger"
                  onClick={() => setConfirmDelete(true)}
                  disabled={deleteVmMutation.isPending}
                >
                  <Trash2 className="w-4 h-4 mr-2" />
                  Delete
                </Button>
              )}
              {confirmDelete && (
                <div className="flex items-center gap-2 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg px-3 py-1.5">
                  <span className="text-xs font-medium text-red-700 dark:text-red-300">Delete?</span>
                  <Button size="sm" variant="danger" className="h-6 px-2 text-xs" onClick={handleDelete}>Confirm</Button>
                  <Button size="sm" variant="ghost" className="h-6 px-2 text-xs" onClick={() => setConfirmDelete(false)}>Cancel</Button>
                </div>
              )}
            </div>
          </div>

          {error && (
            <div className="p-4 flex items-center gap-3 text-sm text-red-600 bg-red-50 border border-red-100 rounded-xl">
              <AlertCircle className="w-5 h-5" />
              <div className="flex-1 font-medium">{error instanceof Error ? error.message : "Failed to load"}</div>
              <Button size="sm" variant="outline" onClick={() => refetch()}>Try again</Button>
            </div>
          )}

          {vm && vm.status.toLowerCase() === "running" && (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mb-8">
              <Card>
                <CardHeader className="py-4 border-b border-zinc-100 dark:border-zinc-800">
                  <CardTitle className="text-xs font-medium text-zinc-500 flex items-center gap-2 uppercase tracking-wider">
                    <Cpu className="w-4 h-4 text-blue-500" /> CPU Usage (%)
                  </CardTitle>
                </CardHeader>
                <CardContent className="pt-6">
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
                    <span className="text-2xl font-bold">{(vm.cpu_usage * 100).toFixed(1)}%</span>
                    <Badge variant="secondary" className="bg-blue-50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400">
                      Real-time
                    </Badge>
                  </div>
                </CardContent>
              </Card>

              <Card>
                <CardHeader className="py-4 border-b border-zinc-100 dark:border-zinc-800">
                  <CardTitle className="text-xs font-medium text-zinc-500 flex items-center gap-2 uppercase tracking-wider">
                    <Zap className="w-4 h-4 text-amber-500" /> Memory Usage (MiB)
                  </CardTitle>
                </CardHeader>
                <CardContent className="pt-6">
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
                    <span className="text-2xl font-bold">{(vm.ram_used_bytes / (1024 * 1024)).toFixed(1)} MiB</span>
                    <Badge variant="secondary" className="bg-amber-50 dark:bg-amber-900/20 text-amber-600 dark:text-amber-400">
                      Live
                    </Badge>
                  </div>
                </CardContent>
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
                    icon={Cpu} 
                    label="CPU Usage" 
                    value={`${cpuPercent.toFixed(1)}%`}
                    percentage={cpuPercent}
                    colorClass={cpuPercent > 80 ? "bg-red-500" : "bg-blue-500"}
                  />
                  <MetricCard 
                    icon={Server} 
                    label="RAM Used" 
                    value={`${ramMiB.toFixed(1)} MB`}
                    percentage={ramPercent}
                    colorClass={ramPercent > 80 ? "bg-red-500" : "bg-green-500"}
                  />
                </div>

                {/* Logs */}
                <Card className="flex flex-col h-[400px]">
                  <CardHeader className="border-b border-zinc-100 dark:border-zinc-800 shrink-0">
                    <CardTitle className="text-base flex items-center gap-2">
                      <Terminal className="w-4 h-4 text-zinc-400" />
                      Live Console Logs
                    </CardTitle>
                  </CardHeader>
                  <CardContent className="p-0 flex-1 min-h-0 bg-zinc-950 overflow-y-auto font-mono text-[10px] sm:text-xs text-zinc-300">
                    <div className="p-4 space-y-1">
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
                  </CardContent>
                </Card>

                <Card>
                  <CardHeader className="border-b border-zinc-100 dark:border-zinc-800">
                    <CardTitle className="text-base flex items-center gap-2">
                      <Terminal className="w-4 h-4 text-zinc-400" />
                      Instance Configuration
                    </CardTitle>
                  </CardHeader>
                  <CardContent className="divide-y divide-zinc-100 dark:divide-zinc-800">
                    <DetailRow icon={Hash} label="Job ID" value={vm.job_id} mono />
                    <DetailRow icon={Server} label="Host Identifier" value={vm.host_id || "Not assigned yet"} mono={!!vm.host_id} />
                    <DetailRow icon={Cpu} label="VM Internal ID" value={vm.vm_id || "Not created yet"} mono={!!vm.vm_id} />
                  </CardContent>
                </Card>
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
                      <div className="absolute left-[31px] top-8 bottom-8 w-px bg-zinc-200 dark:bg-zinc-800" />
                      <div className="relative flex items-center gap-4">
                        <div className="w-4 h-4 rounded-full bg-zinc-200 dark:bg-zinc-800 z-10" />
                        <div>
                          <p className="text-xs font-bold text-zinc-400 uppercase tracking-tighter">Scheduled</p>
                          <p className="text-xs text-zinc-600 dark:text-zinc-400">{formatTimestamp(vm.scheduled_at)}</p>
                        </div>
                      </div>
                      <div className="relative flex items-center gap-4">
                        <div className={cn("w-4 h-4 rounded-full z-10", vm.started_at ? "bg-green-500" : "bg-zinc-200 dark:bg-zinc-800")} />
                        <div>
                          <p className="text-xs font-bold text-zinc-400 uppercase tracking-tighter">Started</p>
                          <p className="text-xs text-zinc-600 dark:text-zinc-400">{formatTimestamp(vm.started_at)}</p>
                        </div>
                      </div>
                      <div className="relative flex items-center gap-4">
                        <div className={cn("w-4 h-4 rounded-full z-10", (vm.stopped_at || !isStoppable(vm.status)) ? "bg-red-500" : "bg-zinc-200 dark:bg-zinc-800")} />
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
