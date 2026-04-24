"use client";

import { useParams } from "next/navigation";
import { 
  HiArrowLeft, 
  HiEye,
  HiEyeOff,
  HiClipboard,
  HiTrash,
  HiCog
} from "react-icons/hi";
import {
  HiCheckCircle, 
  HiExclamationCircle,
  HiRocketLaunch,
  HiInformationCircle
} from "react-icons/hi2";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";

import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { useApps, useDeployments, useActivateDeployment, useDeployAppVersion, useDeleteApp } from "@/lib/hooks/use-apps";
import { useVm } from "@/lib/hooks/use-vms";
import { API_BASE_URL } from "@/lib/api";
import { 
  Badge 
} from "@/components/ui/badge";
import { 
  Table, 
  TableBody, 
  TableCell, 
  TableHead, 
  TableHeader, 
  TableRow 
} from "@/components/ui/table";
import { 
  Alert, 
  AlertDescription, 
  AlertTitle 
} from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { 
  Card, 
  CardHeader, 
  CardTitle, 
  CardContent, 
  CardDescription 
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { 
  Dialog, 
  DialogContent, 
  DialogHeader, 
  DialogTitle, 
  DialogFooter 
} from "@/components/ui/dialog";
import { toast } from "sonner";
import { Loader2 } from "lucide-react";
import { 
  LineChart,
  Line,
  XAxis, 
  YAxis,
  CartesianGrid, 
} from "recharts";
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";
import { cn } from "@/lib/utils";

const chartConfig = {
  cpu: {
    label: "CPU Usage",
    color: "var(--chart-1)",
  },
  ram: {
    label: "RAM Usage",
    color: "var(--chart-2)",
  },
} satisfies ChartConfig;

function getStatusBadgeClass(status: string): string {
  const s = status.toLowerCase();
  if (s === "running") return "bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400 border-green-200 dark:border-green-800";
  if (s === "building" || s === "scheduled" || s === "pending") return "bg-yellow-100 text-yellow-800 dark:bg-yellow-900/30 dark:text-yellow-400 border-yellow-200 dark:border-yellow-800";
  if (s === "failed" || s === "cancelled") return "bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-400 border-red-200 dark:border-red-800";
  return "bg-slate-100 text-slate-800 dark:bg-slate-800 dark:text-slate-400 border-slate-200 dark:border-slate-700";
}

interface MetricPoint {
  time: string;
  cpu: number;
  ram: number;
}

export default function AppDetailPage() {
  const { appId } = useParams() as { appId: string };
  const router = useRouter();
  const { data: apps = [] } = useApps();
  const { data: deployments = [], isLoading, error } = useDeployments(appId);
  const activateMutation = useActivateDeployment();
  const deployAppVersionMutation = useDeployAppVersion();
  const deleteAppMutation = useDeleteApp();

  const [showSecret, setShowSecret] = useState(false);
  const [activeChart, setActiveChart] = useState<"cpu" | "ram">("cpu");
  const [showWebhookModal, setShowWebhookModal] = useState(false);
  const [confirmDeleteApp, setConfirmDeleteApp] = useState(false);

  const app = apps.find(a => a.id === appId);
  // Prefer the active (promoted) deployment, but fallback to the latest running one if none is active
  const activeDeployment = deployments.find(d => d.id === app?.active_deployment_id) 
    || deployments.sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()).find(d => d.status === "RUNNING");
  const activeJobId = activeDeployment?.job_id;

  // Active Instance Logic
  const { data: vm, dataUpdatedAt } = useVm(activeJobId || "");
  const [metricsHistory, setMetricsHistory] = useState<MetricPoint[]>([]);

  useEffect(() => {
    if (!vm) return;

    const timeoutId = setTimeout(() => {
      setMetricsHistory(prev => {
          const newCpu = (vm.cpu_usage || 0) * 100;
          const newRam = (vm.ram_used_bytes || 0) / (1024 * 1024);
          
          // Keep up to 30 points (approx 1.5 min at 3s polling)
          return [...prev.slice(-29), {
              time: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }),
              cpu: newCpu,
              ram: newRam
          }];
      });
    }, 0);

    return () => clearTimeout(timeoutId);
  }, [dataUpdatedAt, vm]);

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
            <Loader2 className="w-8 h-8 animate-spin text-muted-foreground" />
          </div>
        </DashboardLayout>
      </AuthGuard>
    );
  }

  const isInstanceRunning = vm && ["running", "starting", "paused"].includes(vm.status.toLowerCase());
  const latestMetrics = metricsHistory.length > 0 
    ? metricsHistory[metricsHistory.length - 1] 
    : (vm ? { 
        cpu: (vm.cpu_usage || 0) * 100, 
        ram: (vm.ram_used_bytes || 0) / (1024 * 1024) 
      } : { cpu: 0, ram: 0 });

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="space-y-6">
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
            <div className="flex items-center gap-4">
              <div>
                <h1 className="text-2xl font-bold tracking-tight">
                  {app?.name || "Application"} Details
                </h1>
                <p className="text-muted-foreground text-sm mt-1">
                  Manage deployments and monitor production instances.
                </p>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <Button 
                size="sm" 
                onClick={handleDeployApp}
                disabled={deployAppVersionMutation.isPending}
              >
                {deployAppVersionMutation.isPending ? <Loader2 className="w-4 h-4 animate-spin mr-2" /> : <HiRocketLaunch className="w-4 h-4 mr-2" />}
                Deploy Now
              </Button>
              <Button 
                size="sm" 
                variant="outline"
                onClick={() => setShowWebhookModal(true)}
              >
                <HiCog className="w-4 h-4 mr-2" />
                Auto-deploy
              </Button>
              <Button 
                size="sm" 
                variant="destructive"
                onClick={() => confirmDeleteApp ? handleDeleteApp() : setConfirmDeleteApp(true)}
                disabled={deleteAppMutation.isPending}
              >
                {deleteAppMutation.isPending ? <Loader2 className="w-4 h-4 animate-spin mr-2" /> : <HiTrash className="w-4 h-4 mr-2" />}
                {confirmDeleteApp ? "Confirm Delete?" : "Delete App"}
              </Button>
            </div>
          </div>

          <section className="space-y-4">
            <h2 className="text-lg font-bold tracking-tight">
              Deployment History
            </h2>
            <Card className="overflow-hidden">
              {error && (
                <Alert variant="destructive">
                  <HiExclamationCircle className="h-4 w-4" />
                  <AlertDescription>
                    {error instanceof Error ? error.message : "Failed to load deployments"}
                  </AlertDescription>
                </Alert>
              )}

              <div className="overflow-x-auto">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Version / Image</TableHead>
                      <TableHead>Status</TableHead>
                      <TableHead>Created</TableHead>
                      <TableHead>Production</TableHead>
                      <TableHead className="text-right">Actions</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {isLoading && deployments.length === 0 ? (
                      Array.from({ length: 3 }).map((_, i) => (
                        <TableRow key={i}>
                          <TableCell colSpan={5}>
                            <div className="h-8 bg-muted animate-pulse rounded" />
                          </TableCell>
                        </TableRow>
                      ))
                    ) : deployments.length === 0 ? (
                      <TableRow>
                        <TableCell colSpan={5} className="text-center py-10 text-muted-foreground">
                          No deployments found for this application.
                        </TableCell>
                      </TableRow>
                    ) : (
                      deployments.sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()).map((dep) => {
                        const isActive = app?.active_deployment_id === dep.id;
                        const canActivate = dep.status === "RUNNING" && !isActive;

                        return (
                          <TableRow key={dep.id}>
                            <TableCell className="font-medium">
                              <div className="flex flex-col">
                                <span className="text-xs text-muted-foreground font-mono mb-1">{dep.id.split("-")[0]}</span>
                                <span>{dep.image_tag || "N/A"}</span>
                              </div>
                            </TableCell>
                            <TableCell>
                              <Badge className={cn("font-semibold", getStatusBadgeClass(dep.status))}>
                                {dep.status}
                              </Badge>
                            </TableCell>
                            <TableCell className="text-muted-foreground text-xs">
                              {new Date(dep.created_at).toLocaleString()}
                            </TableCell>
                            <TableCell>
                              {isActive ? (
                                <div className="flex items-center gap-1.5 text-green-600 dark:text-green-400 font-semibold text-sm">
                                  <HiCheckCircle className="w-5 h-5" />
                                  <span>Active</span>
                                </div>
                              ) : (
                                <span className="text-muted-foreground text-xs italic">Standby</span>
                              )}
                            </TableCell>
                            <TableCell className="text-right">
                              <Button 
                                size="sm" 
                                variant={isActive ? "outline" : "default"}
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

          {/* Integrated Instance Monitoring */}
          {vm && isInstanceRunning && (
            <div className="space-y-6 animate-in fade-in duration-500 pt-6 border-t">
              <h2 className="text-lg font-bold tracking-tight">
                Live Performance
              </h2>
              <div className="space-y-6">
                {/* Interactive Chart */}
                <Card className="py-4 sm:py-0">
                  <CardHeader className="flex flex-col items-stretch border-b p-0 sm:flex-row">
                    <div className="flex flex-1 flex-col justify-center gap-1 px-6 pb-3 sm:pb-0">
                      <CardTitle>System Performance</CardTitle>
                      <CardDescription>
                        Real-time CPU and RAM utilization
                      </CardDescription>
                    </div>
                    <div className="flex">
                      {(["cpu", "ram"] as const).map((key) => {
                        return (
                          <button
                            key={key}
                            data-active={activeChart === key}
                            className="flex flex-1 flex-col justify-center gap-1 border-t px-6 py-4 text-left even:border-l data-[active=true]:bg-muted/50 sm:border-t-0 sm:border-l sm:px-8 sm:py-6"
                            onClick={() => setActiveChart(key)}
                          >
                            <span className="text-xs text-muted-foreground uppercase">
                              {chartConfig[key].label}
                            </span>
                            <span className="text-lg leading-none font-bold sm:text-3xl">
                              {key === "cpu" 
                                ? `${latestMetrics.cpu.toFixed(1)}%` 
                                : `${latestMetrics.ram.toFixed(0)} MiB`}
                            </span>
                          </button>
                        )
                      })}
                    </div>
                  </CardHeader>
                  <CardContent className="px-2 sm:p-6">
                    <div style={{ width: '100%', height: '250px' }}>
                      <ChartContainer
                        config={chartConfig}
                      >
                        <LineChart
                          data={metricsHistory}
                          margin={{
                            left: 12,
                            right: 12,
                          }}
                        >
                          <CartesianGrid vertical={false} />
                          <XAxis
                            dataKey="time"
                            tickLine={false}
                            axisLine={false}
                            tickMargin={8}
                            minTickGap={32}
                          />
                          <YAxis hide domain={activeChart === "cpu" ? [0, 100] : ['auto', 'auto']} />
                          <ChartTooltip
                            content={
                              <ChartTooltipContent
                                className="w-[150px]"
                                nameKey={activeChart}
                                labelFormatter={(value) => value}
                              />
                            }
                          />
                          <Line
                            dataKey={activeChart}
                            type="monotone"
                            stroke={`var(--color-${activeChart})`}
                            strokeWidth={2}
                            dot={false}
                            isAnimationActive={false}
                          />
                        </LineChart>
                      </ChartContainer>
                    </div>
                  </CardContent>
                </Card>

                {vm.error_message && (
                  <Alert variant="destructive">
                    <HiExclamationCircle className="h-4 w-4" />
                    <AlertTitle className="text-xs font-bold">Termination Error</AlertTitle>
                    <AlertDescription className="text-[10px] break-words">
                      {vm.error_message}
                    </AlertDescription>
                  </Alert>
                )}
              </div>
            </div>
          )}
        </div>

        {/* GitHub Webhook Modal */}
        {showWebhookModal && (
          <Dialog open={true} onOpenChange={(open) => !open && setShowWebhookModal(false)}>
            <DialogContent className="sm:max-w-[600px]">
              <DialogHeader>
                <DialogTitle>GitHub Auto-deploy Configuration</DialogTitle>
              </DialogHeader>
              <div className="space-y-6 pt-4">
                <div className="flex items-start gap-3">
                  <HiInformationCircle className="w-6 h-6 text-indigo-500 mt-0.5 shrink-0" />
                  <div>
                    <p className="text-sm text-muted-foreground">
                      Set up a webhook in your GitHub repository to enable automatic deployments on every push to the <code className="bg-muted px-1 rounded text-foreground">main</code> or <code className="bg-muted px-1 rounded text-foreground">master</code> branch.
                    </p>
                  </div>
                </div>

                <div className="space-y-4 pt-2">
                  <div className="space-y-1.5">
                    <p className="text-[10px] font-bold text-muted-foreground uppercase tracking-wider">Payload URL</p>
                    <div className="flex items-center gap-2">
                      <Input
                        value={`${API_BASE_URL}/webhooks/github/${appId}`}
                        readOnly
                        className="font-mono text-xs flex-1 h-9"
                      />
                      <Button variant="outline" size="sm" className="h-9 px-3" onClick={() => copyToClipboard(`${API_BASE_URL}/webhooks/github/${appId}`)}>
                        <HiClipboard className="w-4 h-4" />
                      </Button>
                    </div>
                  </div>
                  
                  <div className="space-y-1.5">
                    <p className="text-[10px] font-bold text-muted-foreground uppercase tracking-wider">Secret</p>
                    <div className="flex items-center gap-2">
                      <div className="relative flex-1">
                        <Input
                          type={showSecret ? "text" : "password"}
                          value={app?.github_webhook_secret || ""}
                          readOnly
                          className="font-mono text-xs w-full h-9 pr-10"
                        />
                        <button
                          onClick={() => setShowSecret(!showSecret)}
                          className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                        >
                          {showSecret ? <HiEyeOff className="w-4 h-4" /> : <HiEye className="w-4 h-4" />}
                        </button>
                      </div>
                      <Button variant="outline" size="sm" className="h-9 px-3" onClick={() => copyToClipboard(app?.github_webhook_secret || "")}>
                        <HiClipboard className="w-4 h-4" />
                      </Button>
                    </div>
                  </div>
                </div>

                <div className="bg-muted/50 p-4 rounded-lg border">
                  <h4 className="text-xs font-bold mb-2">Instructions:</h4>
                  <ol className="text-xs text-muted-foreground space-y-2 list-decimal list-inside">
                    <li>Go to your repository on GitHub.</li>
                    <li>Click on <span className="font-medium text-foreground">Settings</span> &gt; <span className="font-medium text-foreground">Webhooks</span>.</li>
                    <li>Click <span className="font-medium text-foreground">Add webhook</span>.</li>
                    <li>Paste the <span className="font-medium text-foreground">Payload URL</span> and <span className="font-medium text-foreground">Secret</span> above.</li>
                    <li>Set <span className="font-medium text-foreground">Content type</span> to <span className="font-mono">application/json</span>.</li>
                    <li>Click <span className="font-medium text-foreground">Add webhook</span> at the bottom.</li>
                  </ol>
                </div>
              </div>
              <DialogFooter>
                <Button variant="outline" onClick={() => setShowWebhookModal(false)}>
                  Close
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        )}
      </DashboardLayout>
    </AuthGuard>
  );
}
