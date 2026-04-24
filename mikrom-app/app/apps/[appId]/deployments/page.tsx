"use client";

import { useParams } from "next/navigation";
import { 
  HiArrowLeft, 
  HiEye,
  HiEyeOff,
  HiClipboard,
  HiTrash,
  HiCog,
  HiChip,
  HiServer,
  HiTerminal
} from "react-icons/hi";
import {
  HiCheckCircle, 
  HiExclamationCircle,
  HiRocketLaunch,
  HiInformationCircle
} from "react-icons/hi2";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useCallback, useEffect, useRef, useState, type ElementType } from "react";

import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { useApps, useDeployments, useActivateDeployment, useDeployAppVersion, useDeleteApp } from "@/lib/hooks/use-apps";
import { useVm } from "@/lib/hooks/use-vms";
import { API_BASE_URL, getVmLogsSSE, LogLine } from "@/lib/api";
import { getToken } from "@/lib/auth";
import { Badge, Table, TableBody, TableCell, TableHead, TableHeadCell, TableRow, Alert, Button, Card, TextInput, Modal, ModalHeader, ModalBody, ModalFooter, Progress } from "flowbite-react";
import { toast } from "sonner";
import { Loader2 } from "lucide-react";
import Ansi from "ansi-to-react";
import { 
  AreaChart, 
  Area, 
  XAxis, 
  YAxis, 
  CartesianGrid, 
  Tooltip, 
  ResponsiveContainer 
} from "recharts";

function getStatusColor(status: string): string {
  const s = status.toLowerCase();
  if (s === "running") return "success";
  if (s === "building" || s === "scheduled" || s === "pending") return "warning";
  if (s === "failed" || s === "cancelled") return "failure";
  return "gray";
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
    <Card className="h-full border-none shadow-sm ring-1 ring-zinc-200 dark:ring-zinc-800">
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

export default function AppDeploymentsPage() {
  const { appId } = useParams() as { appId: string };
  const router = useRouter();
  const { data: apps = [] } = useApps();
  const { data: deployments = [], isLoading, error } = useDeployments(appId);
  const activateMutation = useActivateDeployment();
  const deployAppVersionMutation = useDeployAppVersion();
  const deleteAppMutation = useDeleteApp();

  const [showSecret, setShowSecret] = useState(false);
  const [showWebhookModal, setShowWebhookModal] = useState(false);
  const [confirmDeleteApp, setConfirmDeleteApp] = useState(false);

  const app = apps.find(a => a.id === appId);
  const activeDeployment = deployments.find(d => d.id === app?.active_deployment_id);
  const activeJobId = activeDeployment?.job_id;

  // Active Instance Logic
  const { data: vm } = useVm(activeJobId || "");
  const [logs, setLogs] = useState<LogLine[]>([]);
  const [metricsHistory, setMetricsHistory] = useState<MetricPoint[]>([]);
  const logEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (vm?.cpu_usage === undefined || vm?.ram_used_bytes === undefined) return;

    const timeoutId = setTimeout(() => {
        setMetricsHistory(prev => {
            const last = prev[prev.length - 1];
            const newCpu = (vm.cpu_usage || 0) * 100;
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

  useEffect(() => {
    let mounted = true;
    let closeLogs: (() => void) | null = null;

    if (mounted && activeJobId) {
      const token = getToken();
      if (token) {
        closeLogs = getVmLogsSSE(
          token,
          activeJobId,
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
  }, [activeJobId]);

  useEffect(() => {
    scrollToBottom();
  }, [logs.length, scrollToBottom]);

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text);
    toast.success("Copied to clipboard!");
  };

  const handleDeployApp = async () => {
    if (!app) return;
    try {
      const result = await deployAppVersionMutation.mutateAsync({ appId: app.id });
      toast.success(`Deployment for ${app.name} initiated`);
      if (result?.job_id) {
        // Since we are unifying, we stay here. The SSE will update the activeJobId eventually.
      }
    } catch (err) {
      toast.error(`Failed to deploy ${app.name}: ${err instanceof Error ? err.message : "Unknown error"}`);
    }
  };

  const handleDeleteApp = async () => {
    if (!app) return;
    try {
      await deleteAppMutation.mutateAsync(app.id);
      toast.success(`Application ${app.name} deleted`);
      router.push("/apps");
    } catch (err) {
      toast.error(`Failed to delete ${app.name}: ${err instanceof Error ? err.message : "Unknown error"}`);
    }
  };

  const handleActivate = async (deploymentId: string) => {
    toast.promise(activateMutation.mutateAsync({ appId, deploymentId }), {
      loading: "Promoting deployment to production...",
      success: "Deployment activated successfully!",
      error: (err) => `Failed to activate: ${err instanceof Error ? err.message : "Unknown error"}`,
    });
  };

  if (isLoading && apps.length === 0) {
    return (
      <AuthGuard>
        <DashboardLayout>
          <div className="flex items-center justify-center min-h-[400px]">
            <Loader2 className="w-8 h-8 animate-spin text-zinc-400" />
          </div>
        </DashboardLayout>
      </AuthGuard>
    );
  }

  const isInstanceRunning = vm && vm.status.toLowerCase() === "running";

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="space-y-6">
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
            <div className="flex items-center gap-4">
              <Link href="/apps" className="text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100 transition-colors">
                <HiArrowLeft className="w-5 h-5" />
              </Link>
              <div>
                <h1 className="text-2xl font-bold text-zinc-900 dark:text-zinc-50 tracking-tight">
                  {app?.name || "Application"} Details
                </h1>
                <p className="text-zinc-500 dark:text-zinc-400 text-sm mt-1">
                  Manage deployments and monitor production instances.
                </p>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <Button 
                size="sm" 
                color="blue" 
                onClick={handleDeployApp}
                disabled={deployAppVersionMutation.isPending}
              >
                {deployAppVersionMutation.isPending ? <Loader2 className="w-4 h-4 animate-spin mr-2" /> : <HiRocketLaunch className="w-4 h-4 mr-2" />}
                Deploy Now
              </Button>
              <Button 
                size="sm" 
                color="light" 
                onClick={() => setShowWebhookModal(true)}
              >
                <HiCog className="w-4 h-4 mr-2" />
                Auto-deploy
              </Button>
              <Button 
                size="sm" 
                color="failure"
                onClick={() => confirmDeleteApp ? handleDeleteApp() : setConfirmDeleteApp(true)}
                disabled={deleteAppMutation.isPending}
              >
                {deleteAppMutation.isPending ? <Loader2 className="w-4 h-4 animate-spin mr-2" /> : <HiTrash className="w-4 h-4 mr-2" />}
                {confirmDeleteApp ? "Confirm Delete?" : "Delete App"}
              </Button>
            </div>
          </div>

          {/* Integrated Instance Monitoring */}
          {vm && isInstanceRunning && (
            <div className="space-y-6 animate-in fade-in duration-500">
              <div className="space-y-6">
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
                    percentage={(vm.ram_used_bytes / (1024 * 1024 * 512)) * 100} 
                    color="purple"
                  />
                </div>

                {/* Chart */}
                <Card className="p-0 overflow-hidden border-none shadow-sm ring-1 ring-zinc-200 dark:ring-zinc-800">
                  <div className="p-4 border-b border-zinc-100 dark:border-zinc-800">
                    <h5 className="text-sm font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
                      Real-time Performance
                    </h5>
                  </div>
                  <div className="h-48 w-full p-4">
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

                {/* Logs */}
                <Card className="p-0 overflow-hidden bg-zinc-900 border-zinc-800 shadow-xl ring-1 ring-white/10">
                  <div className="p-3 border-b border-zinc-800 flex items-center justify-between bg-zinc-900/50">
                    <div className="flex items-center gap-2">
                      <div className="w-2 h-2 rounded-full bg-green-500 animate-pulse" />
                      <h5 className="text-xs font-semibold uppercase tracking-wider text-zinc-400">
                        Live Instance Logs
                      </h5>
                    </div>
                    <div className="text-[9px] text-zinc-500 font-mono">
                      {logs.length} lines
                    </div>
                  </div>
                  <div className="h-64 overflow-y-auto p-3 font-mono text-[10px] leading-relaxed scrollbar-thin scrollbar-thumb-zinc-700">
                    {logs.length === 0 ? (
                      <div className="flex flex-col items-center justify-center h-full text-zinc-600 italic">
                        <HiTerminal className="w-6 h-6 mb-2 opacity-20" />
                        Waiting for logs...
                      </div>
                    ) : (
                      <div className="space-y-0.5">
                        {logs.map((log, i) => (
                          <div key={i} className="flex gap-3 group">
                            <span className="text-zinc-600 shrink-0 select-none opacity-50 text-[9px]">
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

                {vm.error_message && (
                  <Alert color="failure" icon={HiExclamationCircle} className="dark:bg-red-900/20 dark:text-red-400">
                    <h6 className="font-bold mb-1 text-xs">Termination Error</h6>
                    <p className="text-[10px] break-words">{vm.error_message}</p>
                  </Alert>
                )}
              </div>
            </div>
          )}

          <section className="space-y-4 pt-4 border-t border-zinc-100 dark:border-zinc-800">
            <h2 className="text-lg font-bold text-zinc-900 dark:text-zinc-50 tracking-tight">
              Deployment History
            </h2>
            <Card className="overflow-hidden border-none shadow-sm ring-1 ring-zinc-200 dark:ring-zinc-800">
              {error && (
                <Alert color="failure" icon={() => <HiExclamationCircle className="w-4 h-4 mr-2" />}>
                  {error instanceof Error ? error.message : "Failed to load deployments"}
                </Alert>
              )}

              <div className="overflow-x-auto">
                <Table hoverable>
                  <TableHead>
                    <TableRow>
                      <TableHeadCell>Version / Image</TableHeadCell>
                      <TableHeadCell>Status</TableHeadCell>
                      <TableHeadCell>Created</TableHeadCell>
                      <TableHeadCell>Production</TableHeadCell>
                      <TableHeadCell className="text-right">Actions</TableHeadCell>
                    </TableRow>
                  </TableHead>
                  <TableBody className="divide-y">
                    {isLoading && deployments.length === 0 ? (
                      Array.from({ length: 3 }).map((_, i) => (
                        <TableRow key={i} className="bg-white dark:border-zinc-700 dark:bg-zinc-900">
                          <TableCell colSpan={5}>
                            <div className="h-8 bg-gray-100 dark:bg-gray-800 animate-pulse rounded" />
                          </TableCell>
                        </TableRow>
                      ))
                    ) : deployments.length === 0 ? (
                      <TableRow>
                        <TableCell colSpan={5} className="text-center py-10 text-gray-500">
                          No deployments found for this application.
                        </TableCell>
                      </TableRow>
                    ) : (
                      deployments.sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()).map((dep) => {
                        const isActive = app?.active_deployment_id === dep.id;
                        const canActivate = dep.status === "RUNNING" && !isActive;

                        return (
                          <TableRow key={dep.id} className="bg-white dark:border-zinc-700 dark:bg-zinc-900">
                            <TableCell className="whitespace-nowrap font-medium text-gray-900 dark:text-white">
                              <div className="flex flex-col">
                                <span className="text-xs text-zinc-500 font-mono mb-1">{dep.id.split("-")[0]}</span>
                                <span>{dep.image_tag || "N/A"}</span>
                              </div>
                            </TableCell>
                            <TableCell>
                              <Badge color={getStatusColor(dep.status)} className="w-fit">
                                {dep.status}
                              </Badge>
                            </TableCell>
                            <TableCell className="text-zinc-500 text-xs">
                              {new Date(dep.created_at).toLocaleString()}
                            </TableCell>
                            <TableCell>
                              {isActive ? (
                                <div className="flex items-center gap-1.5 text-green-600 dark:text-green-400 font-semibold text-sm">
                                  <HiCheckCircle className="w-5 h-5" />
                                  <span>Active</span>
                                </div>
                              ) : (
                                <span className="text-zinc-400 text-xs italic">Standby</span>
                              )}
                            </TableCell>
                            <TableCell className="text-right">
                              <Button 
                                size="xs" 
                                color={isActive ? "light" : "success"}
                                disabled={!canActivate || activateMutation.isPending}
                                onClick={() => handleActivate(dep.id)}
                                className="ml-auto"
                              >
                                {isActive ? "Currently in Prod" : "Promote to Prod"}
                                {!isActive && <HiRocketLaunch className="ml-2 w-3 h-3" />}
                              </Button>
                            </TableCell>
                          </TableRow>
                        );
                      })
                    )}
                  </TableBody>
                </Table>
              </div>
            </Card>
          </section>
        </div>

        {/* GitHub Webhook Modal */}
        {showWebhookModal && (
          <Modal show={true} onClose={() => setShowWebhookModal(false)}>
            <ModalHeader>GitHub Auto-deploy Configuration</ModalHeader>
            <ModalBody>
              <div className="space-y-6">
                <div className="flex items-start gap-3">
                  <HiInformationCircle className="w-6 h-6 text-indigo-500 mt-0.5 shrink-0" />
                  <div>
                    <p className="text-sm text-gray-600 dark:text-gray-300">
                      Set up a webhook in your GitHub repository to enable automatic deployments on every push to the <code className="bg-gray-100 dark:bg-gray-800 px-1 rounded">main</code> or <code className="bg-gray-100 dark:bg-gray-800 px-1 rounded">master</code> branch.
                    </p>
                  </div>
                </div>

                <div className="space-y-4 pt-2">
                  <div>
                    <p className="text-[10px] font-bold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-1.5">Payload URL</p>
                    <div className="flex items-center gap-2">
                      <TextInput
                        value={`${API_BASE_URL}/webhooks/github/${appId}`}
                        readOnly
                        sizing="sm"
                        className="font-mono text-xs flex-1"
                      />
                      <Button color="light" size="sm" onClick={() => copyToClipboard(`${API_BASE_URL}/webhooks/github/${appId}`)}>
                        <HiClipboard className="w-4 h-4" />
                      </Button>
                    </div>
                  </div>
                  
                  <div>
                    <p className="text-[10px] font-bold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-1.5">Secret</p>
                    <div className="flex items-center gap-2">
                      <div className="relative flex-1">
                        <TextInput
                          type={showSecret ? "text" : "password"}
                          value={app?.github_webhook_secret || ""}
                          readOnly
                          sizing="sm"
                          className="font-mono text-xs w-full"
                        />
                        <button
                          onClick={() => setShowSecret(!showSecret)}
                          className="absolute right-2 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600 dark:hover:text-gray-200"
                        >
                          {showSecret ? <HiEyeOff className="w-4 h-4" /> : <HiEye className="w-4 h-4" />}
                        </button>
                      </div>
                      <Button color="blue" size="sm" onClick={() => copyToClipboard(app?.github_webhook_secret || "")}>
                        <HiClipboard className="w-4 h-4" />
                      </Button>
                    </div>
                  </div>
                </div>

                <div className="bg-zinc-50 dark:bg-zinc-900/50 p-4 rounded-lg border border-zinc-100 dark:border-zinc-800">
                  <h4 className="text-xs font-bold text-zinc-900 dark:text-zinc-100 mb-2">Instructions:</h4>
                  <ol className="text-xs text-zinc-500 dark:text-zinc-400 space-y-2 list-decimal list-inside">
                    <li>Go to your repository on GitHub.</li>
                    <li>Click on <span className="font-medium text-zinc-700 dark:text-zinc-300">Settings</span> &gt; <span className="font-medium text-zinc-700 dark:text-zinc-300">Webhooks</span>.</li>
                    <li>Click <span className="font-medium text-zinc-700 dark:text-zinc-300">Add webhook</span>.</li>
                    <li>Paste the <span className="font-medium text-zinc-700 dark:text-zinc-300">Payload URL</span> and <span className="font-medium text-zinc-700 dark:text-zinc-300">Secret</span> above.</li>
                    <li>Set <span className="font-medium text-zinc-700 dark:text-zinc-300">Content type</span> to <span className="font-mono">application/json</span>.</li>
                    <li>Click <span className="font-medium text-zinc-700 dark:text-zinc-300">Add webhook</span> at the bottom.</li>
                  </ol>
                </div>
              </div>
            </ModalBody>
            <ModalFooter>
              <div className="flex justify-end w-full">
                <Button color="gray" onClick={() => setShowWebhookModal(false)}>
                  Close
                </Button>
              </div>
            </ModalFooter>
          </Modal>
        )}
      </DashboardLayout>
    </AuthGuard>
  );
}
