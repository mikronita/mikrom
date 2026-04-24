"use client";

import { useCallback, useEffect, useRef, useState, type ElementType, type ReactNode } from "react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { 
  HiChevronLeft, 
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
import { HiRocketLaunch } from "react-icons/hi2";
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
import { getVmLogsSSE, LogLine, pauseVm, resumeVm, activateDeployment } from "@/lib/api";
import { useVm, useStopVm, useDeleteVm } from "@/lib/hooks/use-vms";
import { useApps } from "@/lib/hooks/use-apps";
import Ansi from "ansi-to-react";
import { toast } from "sonner";

import { Badge, Alert, Progress, Button, Card } from "flowbite-react";
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
    <div className="flex items-start gap-4 py-4 border-b border-zinc-100 dark:border-zinc-800 last:border-0">
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
    <Card className="h-full">
      <div className="flex items-center justify-between mb-4">
        <div className="w-8 h-8 rounded-lg bg-zinc-50 dark:bg-zinc-800 flex items-center justify-center border border-zinc-100 dark:border-zinc-700">
          <Icon className="w-4 h-4 text-zinc-500" />
        </div>
        <span className="text-2xl font-bold text-zinc-900 dark:text-white">{value}</span>
      </div>
      <div>
        <div className="flex items-center justify-between mb-1">
          <span className="text-xs font-medium text-zinc-500 dark:text-zinc-400 uppercase tracking-wider">{label}</span>
          <span className="text-xs font-bold text-zinc-900 dark:text-zinc-100">{Math.round(percentage)}%</span>
        </div>
        <Progress progress={percentage} color={color} size="sm" />
      </div>
    </Card>
  );
}

interface MetricPoint {
  time: string;
  cpu: number;
  ram: number;
}

export default function InstanceDetailPage() {
  const params = useParams<{ job_id: string }>();
  const router = useRouter();
  const jobId = params.job_id as string;

  const { data: vm, isLoading, isError } = useVm(jobId);
  const { data: apps = [] } = useApps();

  const stopVmMutation = useStopVm();
  const deleteVmMutation = useDeleteVm();

  const [confirmStop, setConfirmStop] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [metricsHistory, setMetricsHistory] = useState<MetricPoint[]>([]);
  const logEndRef = useRef<HTMLDivElement>(null);

  // Correct way to update history without triggering ESLint cascading renders in React 19
  useEffect(() => {
    if (vm?.cpu_usage === undefined || vm?.ram_used_bytes === undefined) return;

    // Use a small delay to move state update out of the render effect cycle
    const timeoutId = setTimeout(() => {
        setMetricsHistory(prev => {
            const last = prev[prev.length - 1];
            const newCpu = (vm.cpu_usage || 0) * 100;
            
            // Debounce updates if values are identical
            if (last && last.cpu === newCpu && prev.length > 5) return prev;
            
            return [...prev.slice(-19), {
                time: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }),
                cpu: newCpu,
                ram: (vm.ram_used_bytes || 0) / (1024 * 1024)
            }];
        });
    }, 0);

    return () => clearTimeout(timeoutId);
  }, [vm?.cpu_usage, vm?.ram_used_bytes]);

  const scrollToBottom = useCallback(() => {
    logEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, []);

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
        router.push("/deployments");
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
            if (mounted) {
              setLogs(prev => [...prev.slice(-499), log]);
            }
          },
          (err) => {
            console.error("Logs error:", err);
          }
        );
      }
    }

    return () => {
      mounted = false;
      if (closeLogs) closeLogs();
    };
  }, [jobId]);

  useEffect(() => {
    scrollToBottom();
  }, [logs.length, scrollToBottom]);

  if (isLoading) {
    return (
      <DashboardLayout>
        <div className="flex flex-col items-center justify-center min-h-[60vh] gap-4">
          <Loader2 className="w-10 h-10 animate-spin text-indigo-500" />
          <p className="text-zinc-500 animate-pulse">Loading instance details...</p>
        </div>
      </DashboardLayout>
    );
  }

  if (isError || !vm) {
    return (
      <DashboardLayout>
        <Alert color="failure" icon={HiExclamationCircle}>
          <span className="font-medium">Error loading instance:</span> Instance not found
        </Alert>
        <div className="mt-4">
          <Button color="gray" onClick={() => router.push("/deployments")}>
            <HiChevronLeft className="w-4 h-4 mr-2" />
            Back to Deployments
          </Button>
        </div>
      </DashboardLayout>
    );
  }

  const isRunning = vm.status.toLowerCase() === "running";

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="space-y-8">
          {/* Breadcrumbs */}
          <div className="flex items-center gap-2 text-sm text-zinc-500 mb-2">
            <Link href="/deployments" className="hover:text-zinc-900 dark:hover:text-zinc-50 transition-colors flex items-center">
              <HiChevronLeft className="w-4 h-4 mr-1" />
              Deployments
            </Link>
            <span>/</span>
            <span className="text-zinc-900 dark:text-zinc-50 font-medium">Instance Detail</span>
          </div>

          <div className="flex flex-col md:flex-row md:items-center justify-between gap-6">
            <div className="flex items-center gap-4">
              <div className={cn(
                "w-12 h-12 rounded-xl flex items-center justify-center border-2 shadow-sm",
                isRunning 
                  ? "bg-green-50 border-green-200 text-green-600 dark:bg-green-900/20 dark:border-green-800 dark:text-green-400"
                  : "bg-zinc-50 border-zinc-200 text-zinc-400 dark:bg-zinc-800 dark:border-zinc-700"
              )}>
                <HiServer className="w-6 h-6" />
              </div>
              <div>
                <h1 className="text-3xl font-bold text-zinc-900 dark:text-zinc-50 tracking-tight">
                  {vm.app_name}
                </h1>
                <div className="flex items-center gap-2 mt-1">
                  <Badge color={getStatusColor(vm.status)} className="capitalize px-2 py-0.5">
                    {normalizeStatus(vm.status)}
                  </Badge>
                  <span className="text-zinc-400 text-xs flex items-center">
                    <HiHashtag className="w-3 h-3 mr-0.5" />
                    {jobId}
                  </span>
                </div>
              </div>
            </div>

            <div className="flex items-center gap-3">
              {vm && vm.app_id && vm.status.toLowerCase() === "running" && 
               apps.find(a => a.id === vm.app_id)?.active_deployment_id !== vm.deployment_id && (
                <Button 
                  color="dark" 
                  size="sm"
                  onClick={async () => {
                    const deploymentId = vm.deployment_id;
                    const app = apps.find(a => a.id === vm.app_id);
                    if (!deploymentId || !app) return;
                    const res = await activateDeployment(getToken()!, app.id, deploymentId);
                    if (res.error) toast.error(res.error);
                    else { 
                      toast.success("Instance promoted to production!"); 
                      refetch();
                    }
                  }}
                >
                  <HiRocketLaunch className="w-4 h-4 mr-2" />
                  Promote to Production
                </Button>
              )}
              
              {isRunning && (
                <>
                  <Button 
                    color="gray" 
                    size="sm" 
                    onClick={async () => {
                      const res = await pauseVm(getToken()!, jobId);
                      if (res.error) toast.error(res.error);
                      else { toast.success("Instance paused"); refetch(); }
                    }}
                  >
                    <HiPause className="w-4 h-4 mr-2" />
                    Pause
                  </Button>
                  <Button 
                    color="warning" 
                    size="sm" 
                    onClick={() => setConfirmStop(true)}
                  >
                    <HiStop className="w-4 h-4 mr-2" />
                    Stop
                  </Button>
                </>
              )}

              {vm.status.toLowerCase() === "paused" && (
                <Button 
                  color="success" 
                  size="sm" 
                  onClick={async () => {
                    const res = await resumeVm(getToken()!, jobId);
                    if (res.error) toast.error(res.error);
                    else { toast.success("Instance resumed"); refetch(); }
                  }}
                >
                  <HiPlay className="w-4 h-4 mr-2" />
                  Resume
                </Button>
              )}

              {!isRunning && vm.status.toLowerCase() !== "pending" && (
                <Button color="failure" size="sm" onClick={() => setConfirmDelete(true)}>
                  <HiTrash className="w-4 h-4 mr-2" />
                  Delete
                </Button>
              )}
            </div>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
            {/* Left Column: Info and Metrics */}
            <div className="lg:col-span-2 space-y-8">
              {/* Metrics Grid */}
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <MetricCard 
                  icon={HiChip} 
                  label="CPU Usage" 
                  value={`${(vm.cpu_usage * 100).toFixed(1)}%`} 
                  percentage={vm.cpu_usage * 100}
                  color="indigo"
                />
                <MetricCard 
                  icon={HiServer} 
                  label="RAM Usage" 
                  value={`${(vm.ram_used_bytes / (1024 * 1024)).toFixed(0)} MiB`} 
                  percentage={(vm.ram_used_bytes / (1024 * 1024 * 512)) * 100} // Assuming 512MiB max for gauge
                  color="purple"
                />
              </div>

              {/* Chart */}
              <Card className="p-0 overflow-hidden">
                <div className="p-4 border-b border-zinc-100 dark:border-zinc-800">
                  <h5 className="text-sm font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
                    Real-time Performance
                  </h5>
                </div>
                <div className="h-64 w-full p-4">
                  <ResponsiveContainer width="100%" height="100%">
                    <AreaChart data={metricsHistory}>
                      <defs>
                        <linearGradient id="colorCpu" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="5%" stopColor="#6366f1" stopOpacity={0.1}/>
                          <stop offset="95%" stopColor="#6366f1" stopOpacity={0}/>
                        </linearGradient>
                      </defs>
                      <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="#e4e4e7" />
                      <XAxis dataKey="time" hide />
                      <YAxis hide domain={[0, 100]} />
                      <Tooltip 
                        contentStyle={{ 
                          backgroundColor: 'rgba(255, 255, 255, 0.8)', 
                          borderRadius: '8px', 
                          border: 'none',
                          boxShadow: '0 4px 6px -1px rgb(0 0 0 / 0.1)'
                        }} 
                      />
                      <Area 
                        type="monotone" 
                        dataKey="cpu" 
                        stroke="#6366f1" 
                        fillOpacity={1} 
                        fill="url(#colorCpu)" 
                        strokeWidth={2}
                        isAnimationActive={false}
                      />
                    </AreaChart>
                  </ResponsiveContainer>
                </div>
              </Card>

              {/* Console/Logs */}
              <Card className="p-0 overflow-hidden bg-zinc-900 border-zinc-800 shadow-2xl ring-1 ring-white/10">
                <div className="p-4 border-b border-zinc-800 flex items-center justify-between bg-zinc-900/50">
                  <div className="flex items-center gap-2">
                    <div className="w-2 h-2 rounded-full bg-green-500 animate-pulse" />
                    <h5 className="text-sm font-semibold uppercase tracking-wider text-zinc-400">
                      Instance Logs
                    </h5>
                  </div>
                  <div className="text-[10px] text-zinc-500 font-mono">
                    {logs.length} lines captured
                  </div>
                </div>
                <div className="h-96 overflow-y-auto p-4 font-mono text-xs leading-relaxed scrollbar-thin scrollbar-thumb-zinc-700">
                  {logs.length === 0 ? (
                    <div className="flex flex-col items-center justify-center h-full text-zinc-600 italic">
                      <HiTerminal className="w-8 h-8 mb-2 opacity-20" />
                      Waiting for logs...
                    </div>
                  ) : (
                    <div className="space-y-1">
                      {logs.map((log, i) => (
                        <div key={i} className="flex gap-4 group">
                          <span className="text-zinc-600 shrink-0 select-none opacity-50 text-[10px] pt-0.5">
                            {new Date(log.timestamp).toLocaleTimeString([], { hour12: false })}
                          </span>
                          <span className="text-zinc-300 break-all whitespace-pre-wrap">
                            <Ansi>{log.line}</Ansi>
                          </span>
                        </div>
                      ))}
                      <div ref={logEndRef} />
                    </div>
                  )}
                </div>
              </Card>
            </div>

            {/* Right Column: Sidebar info */}
            <div className="space-y-6">
              <Card>
                <h5 className="text-sm font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400 mb-4">
                  Configuration
                </h5>
                <dl className="divide-y divide-zinc-100 dark:divide-zinc-800">
                  <DetailRow icon={HiLightningBolt} label="Instance ID" value={vm.vm_id || "Not assigned"} mono />
                  <DetailRow icon={HiHashtag} label="Job ID" value={jobId} mono />
                  <DetailRow icon={HiServer} label="Worker Node" value={vm.host_id || "Unassigned"} />
                  <DetailRow icon={HiClock} label="Started At" value={formatTimestamp(vm.started_at)} />
                  <DetailRow icon={HiTerminal} label="Image" value={vm.image} mono />
                </dl>
              </Card>

              {vm.error_message && (
                <Alert color="failure" icon={HiExclamationCircle} className="dark:bg-red-900/20 dark:text-red-400">
                  <h6 className="font-bold mb-1">Termination Error</h6>
                  <p className="text-xs break-words">{vm.error_message}</p>
                </Alert>
              )}
            </div>
          </div>
        </div>

        {/* Confirmation Modals */}
        <AppModal show={confirmStop} size="sm">
          <AppModalHeader>Stop Instance</AppModalHeader>
          <AppModalBody>
            <p className="text-sm text-zinc-500">Are you sure you want to stop this instance? Active traffic will be disconnected.</p>
          </AppModalBody>
          <AppModalFooter className="justify-end gap-2">
            <Button color="gray" onClick={() => setConfirmStop(false)}>Cancel</Button>
            <Button color="warning" onClick={handleStop}>Stop Now</Button>
          </AppModalFooter>
        </AppModal>

        <AppModal show={confirmDelete} size="sm">
          <AppModalHeader>Delete Instance</AppModalHeader>
          <AppModalBody>
            <p className="text-sm text-zinc-500">This will permanently remove the instance and its resources. This action cannot be undone.</p>
          </AppModalBody>
          <AppModalFooter className="justify-end gap-2">
            <Button color="gray" onClick={() => setConfirmDelete(false)}>Cancel</Button>
            <Button color="failure" onClick={handleDelete}>Delete Permanently</Button>
          </AppModalFooter>
        </AppModal>
      </DashboardLayout>
    </AuthGuard>
  );
}

// Helper components for Modal to avoid name collision and fix structure
function AppModal({ show, children, size }: { show: boolean, size: string, children: ReactNode }) {
  if (!show) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-zinc-900/50 backdrop-blur-sm">
      <div className={cn("bg-white dark:bg-zinc-900 rounded-xl shadow-2xl ring-1 ring-zinc-200 dark:ring-zinc-800 overflow-hidden", 
        size === "sm" ? "max-w-sm w-full" : "max-w-md w-full")}>
        {children}
      </div>
    </div>
  );
}

function AppModalHeader({ children }: { children: ReactNode }) {
  return (
    <div className="px-6 py-4 border-b border-zinc-100 dark:border-zinc-800">
      <h3 className="text-lg font-bold text-zinc-900 dark:text-white">{children}</h3>
    </div>
  );
}

function AppModalBody({ children }: { children: ReactNode }) {
  return <div className="px-6 py-4">{children}</div>;
}

function AppModalFooter({ className, children }: { className?: string, children: ReactNode }) {
  return <div className={cn("px-6 py-4 bg-zinc-50 dark:bg-zinc-800/50 flex", className)}>{children}</div>;
}
