"use client";

import { useState } from "react";
import Link from "next/link";
import { 
  HiPlus, 
  HiRefresh, 
  HiChartBar, 
  HiCollection, 
  HiLightningBolt, 
  HiServer,
  HiExternalLink,
  HiExclamationCircle,
  HiArrowRight,
  HiStop
} from "react-icons/hi";
import { Loader2 } from "lucide-react";
import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { useVms, useStopVm } from "@/lib/hooks/use-vms";
import { Badge, Alert } from "flowbite-react";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { cn } from "@/lib/utils";
import { DeployModal } from "@/components/DeployModal";
import { toast } from "sonner";

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

export default function Page() {
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
        <div className="space-y-8">
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
              <Button color="gray" size="sm" onClick={() => refetch()} disabled={isFetching}>
                <HiRefresh className={cn("w-4 h-4 mr-2", isFetching && "animate-spin")} />
                Refresh
              </Button>
              <Button size="sm" color="dark" onClick={() => setShowDeploy(true)}>
                <HiPlus className="w-4 h-4 mr-2" />
                Deploy App
              </Button>
            </div>
          </div>

          {/* Stats Grid */}
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6">
            <Card>
              <div className="flex items-center justify-between">
                <h5 className="text-sm font-medium text-gray-500 dark:text-gray-400">Total Apps</h5>
                <HiCollection className="w-4 h-4 text-zinc-500" />
              </div>
              <div className="mt-2">
                {isLoading ? (
                  <div className="h-8 w-12 bg-gray-200 dark:bg-gray-700 animate-pulse rounded" />
                ) : (
                  <div className="text-3xl font-bold dark:text-white">{vms.length}</div>
                )}
                <p className="text-xs text-zinc-500 dark:text-zinc-400 mt-1">
                  Active deployments
                </p>
              </div>
            </Card>
            <Card>
              <div className="flex items-center justify-between">
                <h5 className="text-sm font-medium text-gray-500 dark:text-gray-400">Running</h5>
                <HiChartBar className="w-4 h-4 text-green-500" />
              </div>
              <div className="mt-2">
                {isLoading ? (
                  <div className="h-8 w-12 bg-gray-200 dark:bg-gray-700 animate-pulse rounded" />
                ) : (
                  <div className="text-3xl font-bold text-green-600 dark:text-green-400">{running}</div>
                )}
                <p className="text-xs text-zinc-500 dark:text-zinc-400 mt-1">
                  Currently serving
                </p>
              </div>
            </Card>
            <Card>
              <div className="flex items-center justify-between">
                <h5 className="text-sm font-medium text-gray-500 dark:text-gray-400">Deploying</h5>
                <HiLightningBolt className="w-4 h-4 text-yellow-500" />
              </div>
              <div className="mt-2">
                {isLoading ? (
                  <div className="h-8 w-12 bg-gray-200 dark:bg-gray-700 animate-pulse rounded" />
                ) : (
                  <div className="text-3xl font-bold text-yellow-600 dark:text-yellow-400">{scheduled}</div>
                )}
                <p className="text-xs text-zinc-500 dark:text-zinc-400 mt-1">
                  Pending tasks
                </p>
              </div>
            </Card>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
            {/* Recent Apps */}
            <Card className="lg:col-span-2" noPadding>
              <div className="flex items-center justify-between p-6 pb-0">
                <div>
                  <h5 className="text-xl font-bold dark:text-white">Recent Applications</h5>
                  <p className="text-sm text-gray-500 dark:text-gray-400">Your most recently deployed instances.</p>
                </div>
                <Link href="/vms">
                  <Button color="gray" size="sm">
                    View all
                    <HiArrowRight className="w-3 h-3 ml-2" />
                  </Button>
                </Link>
              </div>
              <div className="mt-6 border-t border-gray-100 dark:border-gray-800">
                {error && (
                  <Alert color="failure" className="rounded-none" icon={() => <HiExclamationCircle className="w-4 h-4 mr-2" />}>
                    {error instanceof Error ? error.message : "Failed to load applications"}
                  </Alert>
                )}

                <div className="divide-y divide-gray-100 dark:divide-gray-800">
                  {isLoading && vms.length === 0 ? (
                    Array.from({ length: 3 }).map((_, i) => (
                      <div key={i} className="px-6 py-4 flex items-center justify-between">
                        <div className="flex items-center gap-4">
                          <div className="w-10 h-10 rounded-lg bg-gray-200 dark:bg-gray-700 animate-pulse" />
                          <div className="space-y-2">
                            <div className="h-4 w-24 bg-gray-200 dark:bg-gray-700 animate-pulse rounded" />
                            <div className="h-3 w-32 bg-gray-200 dark:bg-gray-700 animate-pulse rounded" />
                          </div>
                        </div>
                        <div className="h-6 w-16 bg-gray-200 dark:bg-gray-700 animate-pulse rounded-full" />
                      </div>
                    ))
                  ) : vms.length === 0 && !isLoading ? (
                    <div className="flex flex-col items-center justify-center py-16 text-center">
                      <p className="text-gray-500 dark:text-gray-400 text-sm">No applications found.</p>
                      <Button color="gray" size="sm" className="mt-4" onClick={() => setShowDeploy(true)}>
                        Deploy your first app
                      </Button>
                    </div>
                  ) : (
                    recentVms.map((vm) => (
                      <div
                        key={vm.job_id}
                        className="group px-6 py-4 flex items-center justify-between hover:bg-gray-50 dark:hover:bg-gray-800/30 transition-colors"
                      >
                        <div className="flex items-center gap-4">
                          <div className={cn(
                            "w-10 h-10 rounded-lg flex items-center justify-center border border-gray-200 dark:border-gray-800 bg-white dark:bg-zinc-900 shadow-sm transition-transform group-hover:scale-110",
                            vm.status.toLowerCase() === "running" ? "text-green-600 dark:text-green-400" : "text-gray-400"
                          )}>
                            <HiServer className="w-5 h-5" />
                          </div>
                          <div>
                            <div className="font-semibold text-gray-900 dark:text-white flex items-center gap-2">
                              {vm.app_name}
                              <Badge color={getStatusColor(vm.status)} className="capitalize px-1.5 py-0 h-4 text-[10px]">
                                {normalizeStatus(vm.status)}
                              </Badge>
                            </div>
                            <div className="text-xs text-gray-500 dark:text-gray-400 font-mono mt-0.5 truncate max-w-[150px] sm:max-w-xs">
                              {vm.image}
                            </div>
                          </div>
                        </div>
                        <div className="flex items-center gap-2">
                          {vm.status.toLowerCase() === "running" && (
                            <Button 
                              color="gray" 
                              size="xs"
                              className="opacity-0 group-hover:opacity-100 transition-opacity"
                              onClick={() => handleStopVm(vm.job_id, vm.app_name)}
                              disabled={stopVmMutation.isPending}
                            >
                              {stopVmMutation.isPending ? <Loader2 className="w-4 h-4 animate-spin" /> : <HiStop className="w-4 h-4" />}
                            </Button>
                          )}
                          <Link href={`/vms/${vm.job_id}`}>
                            <Button color="gray" size="xs" className="opacity-0 group-hover:opacity-100 transition-opacity">
                              <HiExternalLink className="w-4 h-4" />
                            </Button>
                          </Link>
                        </div>
                      </div>
                    ))
                  )}
                </div>
              </div>
            </Card>

            {/* Quick Actions / Tips */}
            <Card>
              <h5 className="text-xl font-bold dark:text-white">Quick Actions</h5>
              <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">Common tasks and shortcuts.</p>
              <div className="space-y-3">
                <Button color="gray" outline className="w-full justify-start" onClick={() => setShowDeploy(true)}>
                  <HiPlus className="w-4 h-4 mr-2" />
                  Deploy New App
                </Button>
                <Button color="gray" outline className="w-full justify-start" onClick={() => refetch()} disabled={isFetching}>
                  <HiRefresh className={cn("w-4 h-4 mr-2", isFetching && "animate-spin")} />
                  Sync Resources
                </Button>
                <div className="pt-4 mt-4 border-t border-gray-100 dark:border-gray-800">
                  <h4 className="text-xs font-bold text-gray-400 uppercase tracking-wider mb-3">Resources</h4>
                  <ul className="space-y-2 text-sm text-gray-500">
                    <li><a href="#" className="hover:text-gray-900 dark:hover:text-white flex items-center justify-between">API Reference <HiExternalLink className="w-3 h-3" /></a></li>
                    <li><a href="#" className="hover:text-gray-900 dark:hover:text-white flex items-center justify-between">CLI Tool <HiExternalLink className="w-3 h-3" /></a></li>
                    <li><a href="#" className="hover:text-gray-900 dark:hover:text-white flex items-center justify-between">Firecracker Docs <HiExternalLink className="w-3 h-3" /></a></li>
                  </ul>
                </div>
              </div>
            </Card>
          </div>
        </div>

        {showDeploy && <DeployModal onClose={() => setShowDeploy(false)} />}
      </DashboardLayout>
    </AuthGuard>
  );
}
