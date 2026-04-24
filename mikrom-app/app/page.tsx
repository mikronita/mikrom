"use client";

import { useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { 
  HiPlus, 
  HiChartBar, 
  HiCollection, 
  HiClock,
  HiLightningBolt, 
  HiExternalLink,
  HiExclamationCircle,
  HiArrowRight,
  HiTrash
} from "react-icons/hi";
import { Loader2 } from "lucide-react";
import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { useVms } from "@/lib/hooks/use-vms";
import { useApps, useDeployAppVersion, useDeleteApp } from "@/lib/hooks/use-apps";
import { Alert, Button, Card } from "flowbite-react";
import { cn } from "@/lib/utils";
import { CreateAppModal } from "@/components/CreateAppModal";
import { toast } from "sonner";

function normalizeStatus(status: string): string {
  return status.toLowerCase() === "cancelled" ? "stopped" : status;
}

export default function Page() {
  const router = useRouter();
  const { data: vms = [], isFetching: isFetchingVms, error: vmsError } = useVms();
  const { data: apps = [], isLoading: isLoadingApps, error: appsError } = useApps();
  const deleteAppMutation = useDeleteApp();
  const [showCreateApp, setShowCreateApp] = useState(false);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);

  const deployAppVersionMutation = useDeployAppVersion();

  const handleDeployApp = async (appId: string, appName: string) => {
    try {
      const result = await deployAppVersionMutation.mutateAsync({ appId });
      toast.success(`Deployment for ${appName} initiated`);
      
      if (result?.job_id) {
        router.push(`/deployments/${result.job_id}`);
      } else {
        // If building (no job_id yet), redirect to the history page
        router.push(`/apps/${appId}/deployments`);
      }
    } catch (err) {
      toast.error(`Failed to deploy ${appName}: ${err instanceof Error ? err.message : "Unknown error"}`);
    }
  };

  const handleDeleteApp = (appId: string, appName: string) => {
    setConfirmDeleteId(null);
    toast.promise(deleteAppMutation.mutateAsync(appId), {
      loading: `Deleting application ${appName}...`,
      success: `App ${appName} deleted`,
      error: (err) => `Failed to delete ${appName}: ${err instanceof Error ? err.message : "Unknown error"}`,
    });
  };

  const runningCount = vms.filter((v) => v.status.toLowerCase() === "running").length;
  const pendingCount = vms.filter(
    (v) =>
      v.status.toLowerCase() === "scheduled" ||
      v.status.toLowerCase() === "pending" ||
      v.status.toLowerCase() === "building"
  ).length;

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
              <Button size="sm" color="dark" onClick={() => setShowCreateApp(true)}>
                <HiPlus className="w-4 h-4 mr-2" />
                New Application
              </Button>
            </div>
          </div>

          {/* Stats Grid */}
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6">
            <Card>
              <div className="flex items-center justify-between">
                <h5 className="text-sm font-medium text-gray-500 dark:text-gray-400">Total Applications</h5>
                <HiCollection className="w-4 h-4 text-zinc-500" />
              </div>
              <div className="mt-2">
                {isLoadingApps ? (
                  <div className="h-8 w-12 bg-gray-200 dark:bg-gray-700 animate-pulse rounded" />
                ) : (
                  <div className="text-3xl font-bold dark:text-white">{apps.length}</div>
                )}
                <p className="text-xs text-zinc-500 dark:text-zinc-400 mt-1">
                  Projects in Git
                </p>
              </div>
            </Card>
            <Card>
              <div className="flex items-center justify-between">
                <h5 className="text-sm font-medium text-gray-500 dark:text-gray-400">Running</h5>
                <HiChartBar className="w-4 h-4 text-green-500" />
              </div>
              <div className="mt-2">
                {isFetchingVms && vms.length === 0 ? (
                  <div className="h-8 w-12 bg-gray-200 dark:bg-gray-700 animate-pulse rounded" />
                ) : (
                  <div className="text-3xl font-bold text-green-600 dark:text-green-400">{runningCount}</div>
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
                {isFetchingVms && vms.length === 0 ? (
                  <div className="h-8 w-12 bg-gray-200 dark:bg-gray-700 animate-pulse rounded" />
                ) : (
                  <div className="text-3xl font-bold text-yellow-600 dark:text-yellow-400">{pendingCount}</div>
                )}
                <p className="text-xs text-zinc-500 dark:text-zinc-400 mt-1">
                  Pending tasks
                </p>
              </div>
            </Card>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
            {/* My Apps */}
            <Card className="lg:col-span-2">
              <div className="flex items-center justify-between p-6 pb-0">
                <div>
                  <h5 className="text-xl font-bold dark:text-white">My Applications</h5>
                  <p className="text-sm text-gray-500 dark:text-gray-400">Manage your Git-based projects.</p>
                </div>
                <Button color="gray" size="sm" onClick={() => setShowCreateApp(true)}>
                  <HiPlus className="w-3 h-3 mr-2" />
                  Add App
                </Button>
              </div>
              <div className="mt-6 border-t border-gray-100 dark:border-gray-800">
                {(appsError || vmsError) && (
                  <Alert color="failure" className="rounded-none" icon={() => <HiExclamationCircle className="w-4 h-4 mr-2" />}>
                    {"Failed to load dashboard"}
                  </Alert>
                )}

                <div className="divide-y divide-gray-100 dark:divide-gray-800">
                  {isLoadingApps && apps.length === 0 ? (
                    Array.from({ length: 3 }).map((_, i) => (
                      <div key={i} className="px-6 py-4 flex items-center justify-between">
                        <div className="flex items-center gap-4">
                          <div className="w-10 h-10 rounded-lg bg-gray-200 dark:bg-gray-700 animate-pulse" />
                          <div className="space-y-2">
                            <div className="h-4 w-24 bg-gray-200 dark:bg-gray-700 animate-pulse rounded" />
                            <div className="h-3 w-32 bg-gray-200 dark:bg-gray-700 animate-pulse rounded" />
                          </div>
                        </div>
                      </div>
                    ))
                  ) : apps.length === 0 && !isLoadingApps ? (
                    <div className="flex flex-col items-center justify-center py-16 text-center">
                      <p className="text-gray-500 dark:text-gray-400 text-sm">No applications found.</p>
                      <Button color="gray" size="sm" className="mt-4" onClick={() => setShowCreateApp(true)}>
                        Connect your first repository
                      </Button>
                    </div>
                  ) : (
                    apps.map((app) => (
                      <div
                        key={app.id}
                        className="group px-6 py-4 flex items-center justify-between hover:bg-gray-50 dark:hover:bg-gray-800/30 transition-colors"
                      >
                        <div className="flex items-center gap-4">
                          <div className={cn(
                            "w-10 h-10 rounded-lg flex items-center justify-center border border-gray-200 dark:border-gray-800 bg-white dark:bg-zinc-900 shadow-sm transition-transform group-hover:scale-110 text-indigo-500"
                          )}>
                            <HiCollection className="w-5 h-5" />
                          </div>
                          <div>
                            <div className="font-semibold text-gray-900 dark:text-white flex items-center gap-2">
                              {app.name}
                            </div>
                            <div className="text-xs text-gray-500 dark:text-gray-400 font-mono mt-0.5 truncate max-w-[150px] sm:max-w-xs">
                              {app.git_url}
                            </div>
                          </div>
                        </div>
                        <div className="flex items-center gap-2">
                          <Link href={`/apps/${app.id}/deployments`}>
                            <Button color="gray" size="xs" title="Deployment History">
                              <HiClock className="w-3 h-3 mr-1" /> History
                            </Button>
                          </Link>
                          <Button 
                            color="dark" 
                            size="xs" 
                            onClick={() => handleDeployApp(app.id, app.name)}
                            disabled={deployAppVersionMutation.isPending}
                          >
                            {deployAppVersionMutation.isPending ? <Loader2 className="w-3 h-3 animate-spin" /> : "Deploy"}
                          </Button>
                          <Button 
                            color={confirmDeleteId === app.id ? "failure" : "gray"} 
                            size="xs"
                            onClick={() => confirmDeleteId === app.id ? handleDeleteApp(app.id, app.name) : setConfirmDeleteId(app.id)}
                            disabled={deleteAppMutation.isPending}
                          >
                            {deleteAppMutation.isPending ? <Loader2 className="w-3 h-3 animate-spin" /> : <HiTrash className="w-3 h-3" />}
                            {confirmDeleteId === app.id && <span className="ml-1 text-[10px]">Sure?</span>}
                          </Button>
                        </div>
                      </div>
                    ))
                  )}
                </div>
              </div>
            </Card>

            {/* Active Deployments */}
            <Card>
              <div className="flex items-center justify-between">
                <h5 className="text-lg font-bold dark:text-white">Active Deployments</h5>
                <Link href="/deployments">
                  <HiArrowRight className="w-4 h-4 text-gray-400 hover:text-gray-900 dark:hover:text-white" />
                </Link>
              </div>
              <div className="mt-4 space-y-4">
                {isFetchingVms && vms.length === 0 ? (
                  Array.from({ length: 2 }).map((_, i) => (
                    <div key={i} className="h-12 bg-gray-100 dark:bg-gray-800 animate-pulse rounded-lg" />
                  ))
                ) : vms.length === 0 ? (
                  <p className="text-xs text-zinc-500 text-center py-4">No active deployments.</p>
                ) : (
                  vms.slice(0, 5).map((vm) => (
                    <Link key={vm.job_id} href={`/deployments/${vm.job_id}`} className="block group">
                      <div className="flex items-center gap-3 p-2 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-800/50 transition-colors">
                        <div className={cn(
                          "w-2 h-2 rounded-full",
                          vm.status.toLowerCase() === "running" ? "bg-green-500" : "bg-yellow-500"
                        )} />
                        <div className="flex-1 min-w-0">
                          <p className="text-sm font-medium text-zinc-900 dark:text-white truncate">{vm.app_name}</p>
                          <div className="flex items-center gap-2">
                            <p className="text-[10px] text-zinc-500 truncate uppercase">{normalizeStatus(vm.status)}</p>
                            {apps.find(a => a.id === vm.app_id)?.active_deployment_id === vm.deployment_id && (
                              <span className="text-[8px] bg-green-100 text-green-700 px-1 rounded font-bold uppercase">Prod</span>
                            )}
                          </div>
                        </div>
                        <HiExternalLink className="w-3 h-3 text-zinc-300 group-hover:text-zinc-500" />
                      </div>
                    </Link>
                  ))
                )}
              </div>
            </Card>
          </div>
        </div>

        {showCreateApp && <CreateAppModal onClose={() => setShowCreateApp(false)} />}
      </DashboardLayout>
    </AuthGuard>
  );
}
