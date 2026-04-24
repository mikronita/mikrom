"use client";

import { useState } from "react";
import Link from "next/link";
import { 
  HiPlus, 
  HiCollection, 
  HiExclamationCircle
} from "react-icons/hi";
import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { useApps } from "@/lib/hooks/use-apps";
import { useVms } from "@/lib/hooks/use-vms";
import { Alert, Button, Card } from "flowbite-react";
import { cn } from "@/lib/utils";
import { CreateAppModal } from "@/components/CreateAppModal";

export default function ApplicationsPage() {
  const { data: apps = [], isLoading: isLoadingApps, error: appsError } = useApps();
  const { error: vmsError } = useVms();
  const [showCreateApp, setShowCreateApp] = useState(false);

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="space-y-6">
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
            <div>
              <h1 className="text-3xl font-bold text-zinc-900 dark:text-zinc-50 tracking-tight">
                Applications
              </h1>
              <p className="text-zinc-500 dark:text-zinc-400 mt-1">
                Manage your Git-based projects and deployments.
              </p>
            </div>
            <div className="flex items-center gap-3">
              <Button size="sm" color="blue" onClick={() => setShowCreateApp(true)}>
                <HiPlus className="w-4 h-4 mr-2" />
                New Application
              </Button>
            </div>
          </div>

          <Card className="overflow-hidden border-none shadow-sm ring-1 ring-zinc-200 dark:ring-zinc-800">
            <div className="flex items-center justify-between p-6 pb-0">
              <div>
                <h5 className="text-xl font-bold dark:text-white">My Applications</h5>
                <p className="text-sm text-gray-500 dark:text-gray-400">All registered projects.</p>
              </div>
            </div>
            <div className="mt-6 border-t border-gray-100 dark:border-gray-800">
              {(appsError || vmsError) && (
                <Alert color="failure" className="rounded-none" icon={HiExclamationCircle}>
                  <span>{appsError?.message || vmsError?.message || "Failed to load applications"}</span>
                </Alert>
              )}

              <div className="divide-y divide-gray-100 dark:divide-gray-800">
                {isLoadingApps && apps.length === 0 ? (
                  Array.from({ length: 5 }).map((_, i) => (
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
                    <Button color="blue" size="sm" className="mt-4" onClick={() => setShowCreateApp(true)}>
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
                          <Button color="light" size="sm">
                            Manage
                          </Button>
                        </Link>
                      </div>
                    </div>
                  ))
                )}
              </div>
            </div>
          </Card>
        </div>

        {showCreateApp && <CreateAppModal onClose={() => setShowCreateApp(false)} />}
      </DashboardLayout>
    </AuthGuard>
  );
}
