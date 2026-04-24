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
import { Button, Card } from "flowbite-react";
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
              <h1 className="text-3xl font-bold text-zinc-900 dark:text-zinc-50 tracking-tight">
                Dashboard
              </h1>
              <p className="text-zinc-500 dark:text-zinc-400 mt-1">
                Overview of your cloud resources and applications.
              </p>
            </div>
            <div className="flex items-center gap-3">
              <Button size="sm" color="blue" onClick={() => setShowCreateApp(true)}>
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
            <Card className="lg:col-span-3">
              <div className="flex items-center justify-between p-6">
                <div>
                  <h5 className="text-xl font-bold dark:text-white">Welcome back!</h5>
                  <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
                    Your cloud infrastructure is running smoothly. 
                  </p>
                </div>
                <Link href="/apps">
                  <Button color="blue" size="sm">
                    <HiCollection className="w-4 h-4 mr-2" />
                    View All Applications
                  </Button>
                </Link>
              </div>
            </Card>
          </div>
        </div>

        {showCreateApp && <CreateAppModal onClose={() => setShowCreateApp(false)} />}
      </DashboardLayout>
    </AuthGuard>
  );
}
