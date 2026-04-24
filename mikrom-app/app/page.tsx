"use client";

import { useState } from "react";
import Link from "next/link";
import { 
  HiPlus, 
  HiChartBar, 
  HiCollection, 
  HiLightningBolt
} from "react-icons/hi";
import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { useVms } from "@/lib/hooks/use-vms";
import { useApps } from "@/lib/hooks/use-apps";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { CreateAppModal } from "@/components/CreateAppModal";

export default function Page() {
  const { data: vms = [], isFetching: isFetchingVms } = useVms();
  const { data: apps = [], isLoading: isLoadingApps } = useApps();
  const [showCreateApp, setShowCreateApp] = useState(false);

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
              <h1 className="text-3xl font-bold tracking-tight">
                Dashboard
              </h1>
              <p className="text-muted-foreground mt-1">
                Overview of your cloud resources and applications.
              </p>
            </div>
            <div className="flex items-center gap-3">
              <Button size="sm" onClick={() => setShowCreateApp(true)}>
                <HiPlus className="w-4 h-4 mr-2" />
                New Application
              </Button>
            </div>
          </div>

          {/* Stats Grid */}
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6">
            <Card>
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">Total Applications</CardTitle>
                <HiCollection className="w-4 h-4 text-muted-foreground" />
              </CardHeader>
              <CardContent>
                {isLoadingApps ? (
                  <div className="h-8 w-12 bg-muted animate-pulse rounded" />
                ) : (
                  <div className="text-2xl font-bold">{apps.length}</div>
                )}
                <p className="text-xs text-muted-foreground mt-1">
                  Projects in Git
                </p>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">Running</CardTitle>
                <HiChartBar className="w-4 h-4 text-green-500" />
              </CardHeader>
              <CardContent>
                {isFetchingVms && vms.length === 0 ? (
                  <div className="h-8 w-12 bg-muted animate-pulse rounded" />
                ) : (
                  <div className="text-2xl font-bold text-green-600 dark:text-green-400">{runningCount}</div>
                )}
                <p className="text-xs text-muted-foreground mt-1">
                  Currently serving
                </p>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">Deploying</CardTitle>
                <HiLightningBolt className="w-4 h-4 text-yellow-500" />
              </CardHeader>
              <CardContent>
                {isFetchingVms && vms.length === 0 ? (
                  <div className="h-8 w-12 bg-muted animate-pulse rounded" />
                ) : (
                  <div className="text-2xl font-bold text-yellow-600 dark:text-yellow-400">{pendingCount}</div>
                )}
                <p className="text-xs text-muted-foreground mt-1">
                  Pending tasks
                </p>
              </CardContent>
            </Card>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
            <Card className="lg:col-span-3">
              <CardContent className="flex items-center justify-between p-6">
                <div>
                  <h3 className="text-xl font-bold">Welcome back!</h3>
                  <p className="text-sm text-muted-foreground mt-1">
                    Your cloud infrastructure is running smoothly. 
                  </p>
                </div>
                <Link href="/apps">
                  <Button size="sm" variant="outline">
                    <HiCollection className="w-4 h-4 mr-2" />
                    View All Applications
                  </Button>
                </Link>
              </CardContent>
            </Card>
          </div>
        </div>

        {showCreateApp && <CreateAppModal onClose={() => setShowCreateApp(false)} />}
      </DashboardLayout>
    </AuthGuard>
  );
}
