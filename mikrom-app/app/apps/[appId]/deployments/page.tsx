"use client";

import { useParams } from "next/navigation";
import { 
  HiArrowLeft, 
  HiRefresh, 
} from "react-icons/hi";
import {
  HiCheckCircle, 
  HiExclamationCircle,
  HiRocketLaunch
} from "react-icons/hi2";
import Link from "next/link";

import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { useApps, useDeployments, useActivateDeployment } from "@/lib/hooks/use-apps";
import { Badge, Table, TableBody, TableCell, TableHead, TableHeadCell, TableRow, Alert, Button, Card } from "flowbite-react";
import { cn } from "@/lib/utils";
import { toast } from "sonner";

function getStatusColor(status: string): string {
  const s = status.toLowerCase();
  if (s === "running") return "success";
  if (s === "building" || s === "pending") return "warning";
  if (s === "failed") return "failure";
  return "gray";
}

export default function AppDeploymentsPage() {
  const { appId } = useParams() as { appId: string };
  const { data: apps = [] } = useApps();
  const { data: deployments = [], isLoading, isFetching, refetch, error } = useDeployments(appId);
  const activateMutation = useActivateDeployment();

  const app = apps.find(a => a.id === appId);

  const handleActivate = async (deploymentId: string) => {
    toast.promise(activateMutation.mutateAsync({ appId, deploymentId }), {
      loading: "Promoting deployment to production...",
      success: "Deployment activated successfully!",
      error: (err) => `Failed to activate: ${err instanceof Error ? err.message : "Unknown error"}`,
    });
  };

  if (!app && apps.length > 0) {
    return (
      <DashboardLayout>
        <Alert color="failure">Application not found.</Alert>
      </DashboardLayout>
    );
  }

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="space-y-6">
          <div className="flex items-center gap-4">
            <Link href="/" className="text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100 transition-colors">
              <HiArrowLeft className="w-5 h-5" />
            </Link>
            <div className="flex-1">
              <h1 className="text-2xl font-bold text-zinc-900 dark:text-zinc-50 tracking-tight">
                {app?.name || "Application"} Deployments
              </h1>
              <p className="text-zinc-500 dark:text-zinc-400 text-sm mt-1">
                Version history and production promotion.
              </p>
            </div>
            <Button color="gray" size="sm" onClick={() => refetch()} disabled={isFetching}>
              <HiRefresh className={cn("w-4 h-4 mr-2", isFetching && "animate-spin")} />
              Refresh
            </Button>
          </div>

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
                              color="dark"
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
        </div>
      </DashboardLayout>
    </AuthGuard>
  );
}
