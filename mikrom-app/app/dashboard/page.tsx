"use client";

import { useState } from "react";
import Link from "next/link";
import { 
  Plus, 
  RefreshCw, 
  Activity, 
  Layers, 
  Zap, 
  Server,
  ExternalLink,
  AlertCircle,
  ArrowRight,
  Square,
  Loader2
} from "lucide-react";

import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { useVms, useStopVm } from "@/lib/hooks/use-vms";

import { Button } from "@/components/ui/Button";
import { Badge } from "@/components/ui/Badge";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/Card";
import { Skeleton } from "@/components/ui/Skeleton";
import { cn } from "@/lib/utils";
import { DeployModal } from "@/components/DeployModal";
import { toast } from "sonner";

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

export default function DashboardPage() {
  const { data: vms = [], isLoading, error, refetch, isFetching } = useVms();
  const stopVmMutation = useStopVm();
  const [showDeploy, setShowDeploy] = useState(false);

  const handleStopVm = (jobId: string, appName: string) => {
    toast.promise(stopVmMutation.mutateAsync(jobId), {
      loading: `Stopping ${appName}...`,
      success: `App ${appName} stopped successfully`,
      error: (err) => `Failed to stop ${appName}: ${err instanceof Error ? err.message : "Unknown error"}`,
    });
  };

  const running = vms.filter((v) => v.status.toLowerCase() === "running").length;
  const scheduled = vms.filter(
    (v) =>
      v.status.toLowerCase() === "scheduled" ||
      v.status.toLowerCase() === "pending"
  ).length;

  const recentVms = vms.slice(0, 5);

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="p-8 space-y-8">
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
            <div>
              <h1 className="text-3xl font-bold text-zinc-900 dark:text-zinc-50 tracking-tight">
                Dashboard
              </h1>
              <p className="text-zinc-500 dark:text-zinc-400 mt-1">
                Overview of your cloud resources and applications.
              </p>
            </div>
            <div className="flex items-center gap-3">
              <Button variant="outline" size="sm" onClick={() => refetch()} disabled={isFetching}>
                <RefreshCw className={cn("w-4 h-4 mr-2", isFetching && "animate-spin")} />
                Refresh
              </Button>
              <Button size="sm" onClick={() => setShowDeploy(true)}>
                <Plus className="w-4 h-4 mr-2" />
                Deploy App
              </Button>
            </div>
          </div>

          {/* Stats Grid */}
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6">
            <Card>
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">Total Apps</CardTitle>
                <Layers className="w-4 h-4 text-zinc-500" />
              </CardHeader>
              <CardContent>
                {isLoading ? <Skeleton className="h-8 w-12" /> : <div className="text-2xl font-bold">{vms.length}</div>}
                <p className="text-xs text-zinc-500 dark:text-zinc-400 mt-1">
                  Active deployments
                </p>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">Running</CardTitle>
                <Activity className="w-4 h-4 text-green-500" />
              </CardHeader>
              <CardContent>
                {isLoading ? <Skeleton className="h-8 w-12" /> : <div className="text-2xl font-bold text-green-600 dark:text-green-400">{running}</div>}
                <p className="text-xs text-zinc-500 dark:text-zinc-400 mt-1">
                  Currently serving
                </p>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">Deploying</CardTitle>
                <Zap className="w-4 h-4 text-yellow-500" />
              </CardHeader>
              <CardContent>
                {isLoading ? <Skeleton className="h-8 w-12" /> : <div className="text-2xl font-bold text-yellow-600 dark:text-yellow-400">{scheduled}</div>}
                <p className="text-xs text-zinc-500 dark:text-zinc-400 mt-1">
                  Pending tasks
                </p>
              </CardContent>
            </Card>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
            {/* Recent Apps */}
            <Card className="lg:col-span-2 overflow-hidden">
              <CardHeader className="flex flex-row items-center justify-between">
                <div>
                  <CardTitle>Recent Applications</CardTitle>
                  <CardDescription>Your most recently deployed instances.</CardDescription>
                </div>
                <Link href="/dashboard/vms">
                  <Button variant="ghost" size="sm" className="text-zinc-500">
                    View all
                    <ArrowRight className="w-3 h-3 ml-2" />
                  </Button>
                </Link>
              </CardHeader>
              <CardContent className="p-0 border-t border-zinc-100 dark:border-zinc-800">
                {error && (
                  <div className="p-4 flex items-center gap-3 text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/10">
                    <AlertCircle className="w-4 h-4" />
                    {error instanceof Error ? error.message : "Failed to load applications"}
                  </div>
                )}

                <div className="divide-y divide-zinc-100 dark:divide-zinc-800">
                  {isLoading && vms.length === 0 ? (
                    Array.from({ length: 3 }).map((_, i) => (
                      <div key={i} className="px-6 py-4 flex items-center justify-between">
                        <div className="flex items-center gap-4">
                          <Skeleton className="w-10 h-10 rounded-lg" />
                          <div className="space-y-2">
                            <Skeleton className="h-4 w-24" />
                            <Skeleton className="h-3 w-32" />
                          </div>
                        </div>
                        <Skeleton className="h-6 w-16 rounded-full" />
                      </div>
                    ))
                  ) : vms.length === 0 && !isLoading ? (
                    <div className="flex flex-col items-center justify-center py-16 text-center">
                      <p className="text-zinc-500 dark:text-zinc-400 text-sm">No applications found.</p>
                      <Button variant="outline" size="sm" className="mt-4" onClick={() => setShowDeploy(true)}>
                        Deploy your first app
                      </Button>
                    </div>
                  ) : (
                    recentVms.map((vm) => (
                      <div
                        key={vm.job_id}
                        className="group px-6 py-4 flex items-center justify-between hover:bg-zinc-50 dark:hover:bg-zinc-800/30 transition-colors"
                      >
                        <div className="flex items-center gap-4">
                          <div className={cn(
                            "w-10 h-10 rounded-lg flex items-center justify-center border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-sm transition-transform group-hover:scale-110",
                            vm.status.toLowerCase() === "running" ? "text-green-600 dark:text-green-400" : "text-zinc-400"
                          )}>
                            <Server className="w-5 h-5" />
                          </div>
                          <div>
                            <div className="font-semibold text-zinc-900 dark:text-zinc-100 flex items-center gap-2">
                              {vm.app_name}
                              <Badge variant={getStatusVariant(vm.status)} className="capitalize px-1.5 py-0 h-4 text-[10px]">
                                {normalizeStatus(vm.status)}
                              </Badge>
                            </div>
                            <div className="text-xs text-zinc-500 dark:text-zinc-400 font-mono mt-0.5 truncate max-w-[150px] sm:max-w-xs">
                              {vm.image}
                            </div>
                          </div>
                        </div>
                        <div className="flex items-center gap-2">
                          {vm.status.toLowerCase() === "running" && (
                            <Button 
                              variant="ghost" 
                              size="icon" 
                              className="h-8 w-8 text-zinc-400 hover:text-red-500 opacity-0 group-hover:opacity-100 transition-opacity"
                              onClick={() => handleStopVm(vm.job_id, vm.app_name)}
                              disabled={stopVmMutation.isPending}
                            >
                              {stopVmMutation.isPending ? <Loader2 className="w-4 h-4 animate-spin" /> : <Square className="w-4 h-4" />}
                            </Button>
                          )}
                          <Link href={`/dashboard/vms/${vm.job_id}`}>
                            <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity">
                              <ExternalLink className="w-4 h-4" />
                            </Button>
                          </Link>
                        </div>
                      </div>
                    ))
                  )}
                </div>
              </CardContent>
            </Card>

            {/* Quick Actions / Tips */}
            <Card>
              <CardHeader>
                <CardTitle>Quick Actions</CardTitle>
                <CardDescription>Common tasks and shortcuts.</CardDescription>
              </CardHeader>
              <CardContent className="space-y-3">
                <Button variant="outline" className="w-full justify-start gap-3" onClick={() => setShowDeploy(true)}>
                  <Plus className="w-4 h-4" />
                  Deploy New App
                </Button>
                <Button variant="outline" className="w-full justify-start gap-3" onClick={() => refetch()} disabled={isFetching}>
                  <RefreshCw className={cn("w-4 h-4", isFetching && "animate-spin")} />
                  Sync Resources
                </Button>
                <div className="pt-4 mt-4 border-t border-zinc-100 dark:border-zinc-800">
                  <h4 className="text-xs font-bold text-zinc-400 uppercase tracking-wider mb-3">Resources</h4>
                  <ul className="space-y-2 text-sm text-zinc-500">
                    <li><a href="#" className="hover:text-zinc-900 dark:hover:text-zinc-100 flex items-center justify-between">API Reference <ExternalLink className="w-3 h-3" /></a></li>
                    <li><a href="#" className="hover:text-zinc-900 dark:hover:text-zinc-100 flex items-center justify-between">CLI Tool <ExternalLink className="w-3 h-3" /></a></li>
                    <li><a href="#" className="hover:text-zinc-900 dark:hover:text-zinc-100 flex items-center justify-between">Firecracker Docs <ExternalLink className="w-3 h-3" /></a></li>
                  </ul>
                </div>
              </CardContent>
            </Card>
          </div>
        </div>

        {showDeploy && <DeployModal onClose={() => setShowDeploy(false)} />}
      </DashboardLayout>
    </AuthGuard>
  );
}
