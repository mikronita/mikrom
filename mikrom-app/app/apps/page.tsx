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
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle, CardDescription, CardContent } from "@/components/ui/card";
import { Alert, AlertDescription } from "@/components/ui/alert";
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
              <h1 className="text-3xl font-bold tracking-tight">
                Applications
              </h1>
              <p className="text-muted-foreground mt-1">
                Manage your Git-based projects and deployments.
              </p>
            </div>
            <div className="flex items-center gap-3">
              <Button size="sm" onClick={() => setShowCreateApp(true)}>
                <HiPlus className="w-4 h-4 mr-2" />
                New Application
              </Button>
            </div>
          </div>

          <Card className="overflow-hidden">
            <CardHeader className="p-6 pb-0">
              <CardTitle className="text-xl font-bold">My Applications</CardTitle>
              <CardDescription>All registered projects.</CardDescription>
            </CardHeader>
            <CardContent className="p-0 mt-6 border-t">
              {(appsError || vmsError) && (
                <Alert variant="destructive" className="rounded-none border-x-0 border-t-0">
                  <HiExclamationCircle className="h-4 w-4" />
                  <AlertDescription>
                    {appsError?.message || vmsError?.message || "Failed to load applications"}
                  </AlertDescription>
                </Alert>
              )}

              <div className="divide-y">
                {isLoadingApps && apps.length === 0 ? (
                  Array.from({ length: 5 }).map((_, i) => (
                    <div key={i} className="px-6 py-4 flex items-center justify-between">
                      <div className="flex items-center gap-4">
                        <div className="w-10 h-10 rounded-lg bg-muted animate-pulse" />
                        <div className="space-y-2">
                          <div className="h-4 w-24 bg-muted animate-pulse rounded" />
                          <div className="h-3 w-32 bg-muted animate-pulse rounded" />
                        </div>
                      </div>
                    </div>
                  ))
                ) : apps.length === 0 && !isLoadingApps ? (
                  <div className="flex flex-col items-center justify-center py-16 text-center">
                    <p className="text-muted-foreground text-sm">No applications found.</p>
                    <Button size="sm" className="mt-4" onClick={() => setShowCreateApp(true)}>
                      Connect your first repository
                    </Button>
                  </div>
                ) : (
                  apps.map((app) => (
                    <div
                      key={app.id}
                      className="group px-6 py-4 flex items-center justify-between hover:bg-muted/30 transition-colors"
                    >
                      <div className="flex items-center gap-4">
                        <div className={cn(
                          "w-10 h-10 rounded-lg flex items-center justify-center border bg-card shadow-sm transition-transform group-hover:scale-110 text-indigo-500"
                        )}>
                          <HiCollection className="w-5 h-5" />
                        </div>
                        <div>
                          <div className="font-semibold flex items-center gap-2">
                            {app.name}
                          </div>
                          <div className="text-xs text-muted-foreground font-mono mt-0.5 truncate max-w-[150px] sm:max-w-xs">
                            {app.git_url}
                          </div>
                        </div>
                      </div>
                      <div className="flex items-center gap-2">
                        <Link href={`/apps/${app.id}`}>
                          <Button variant="outline" size="sm">
                            Manage
                          </Button>
                        </Link>
                      </div>
                    </div>
                  ))
                )}
              </div>
            </CardContent>
          </Card>
        </div>

        {showCreateApp && <CreateAppModal onClose={() => setShowCreateApp(false)} />}
      </DashboardLayout>
    </AuthGuard>
  );
}
