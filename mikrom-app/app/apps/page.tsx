"use client";

import { useState } from "react";
import Link from "next/link";
import { Calendar, Cpu, ExternalLink, FolderPlus, GitBranch, HardDrive, Plus, TriangleAlert } from "lucide-react";
import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { useApps } from "@/lib/hooks/use-apps";
import { useVms, useWatchVms } from "@/lib/hooks/use-vms";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle, CardContent, CardDescription } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { 
  Empty, 
  EmptyContent, 
  EmptyDescription, 
  EmptyHeader, 
  EmptyMedia, 
  EmptyTitle 
} from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";
import { CreateAppModal } from "@/components/CreateAppModal";

const formatDate = (dateStr: string) => {
  try {
    return new Intl.DateTimeFormat('en-US', {
      month: 'short',
      day: 'numeric',
      year: 'numeric'
    }).format(new Date(dateStr));
  } catch {
    return dateStr;
  }
};

export default function ApplicationsPage() {
  const { data: apps = [], isLoading: isLoadingApps, error: appsError } = useApps();
  const { data: vms = [], error: vmsError } = useVms();
  useWatchVms();
  const [showCreateApp, setShowCreateApp] = useState(false);

  // Optimize VM lookup by creating a Map
  const vmsMap = new Map(vms.map(vm => [vm.app_id || vm.app_name, vm]));

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="flex flex-col gap-6">
          <div className="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
            <div className="flex flex-col gap-2">
              <h1 className="text-3xl font-semibold tracking-tight">
                Applications
              </h1>
              <p className="max-w-2xl text-sm text-muted-foreground">
                Manage your Git-based projects and deployments.
              </p>
            </div>
            <Button onClick={() => setShowCreateApp(true)}>
              <Plus data-icon="inline-start" />
              New Application
            </Button>
          </div>

          {(appsError || vmsError) && (
            <Alert variant="destructive">
              <TriangleAlert />
              <AlertDescription>
                {appsError?.message || vmsError?.message || "Failed to load applications"}
              </AlertDescription>
            </Alert>
          )}

          <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
            {isLoadingApps && apps.length === 0 ? (
              Array.from({ length: 6 }).map((_, i) => (
                <Card key={i}>
                  <CardHeader>
                    <div className="flex items-start gap-4">
                      <Skeleton className="size-10 rounded-lg" />
                      <div className="flex flex-1 flex-col gap-2">
                        <Skeleton className="h-5 w-32" />
                        <Skeleton className="h-4 w-full" />
                      </div>
                    </div>
                  </CardHeader>
                  <CardContent className="flex flex-col gap-3">
                    <Skeleton className="h-4 w-40" />
                    <Skeleton className="h-4 w-28" />
                  </CardContent>
                </Card>
              ))
            ) : apps.length === 0 && !isLoadingApps ? (
              <div className="col-span-full">
                <Empty className="py-16">
                  <EmptyHeader>
                    <EmptyMedia variant="icon">
                      <FolderPlus />
                    </EmptyMedia>
                    <EmptyTitle>No applications found</EmptyTitle>
                    <EmptyDescription>
                      Get started by connecting your first repository.
                    </EmptyDescription>
                  </EmptyHeader>
                  <EmptyContent>
                    <Button size="sm" onClick={() => setShowCreateApp(true)}>
                      <Plus data-icon="inline-start" />
                      Connect your first repository
                    </Button>
                  </EmptyContent>
                </Empty>
              </div>
            ) : (
              [...apps].sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()).map((app) => {
                const appVm = vmsMap.get(app.id) || vmsMap.get(app.name);
                const isRunning = !!appVm && appVm.status === "running";

                return (
                  <Link 
                    key={app.id} 
                    href={`/apps/${encodeURIComponent(app.name)}`}
                    className="block"
                  >
                    <Card className="h-full transition-colors hover:bg-muted/30">
                      <CardHeader>
                        <div className="flex items-start gap-4">
                          <div className="flex size-10 shrink-0 items-center justify-center rounded-lg border bg-muted text-muted-foreground">
                            <GitBranch />
                          </div>
                          <div className="flex min-w-0 flex-1 flex-col gap-2">
                            <div className="flex min-w-0 items-center gap-2">
                              <CardTitle className="truncate text-base">
                                {app.name}
                              </CardTitle>
                              {isRunning && <Badge variant="success" className="uppercase">Live</Badge>}
                            </div>
                            <CardDescription className="truncate font-mono text-xs">
                              {app.git_url}
                            </CardDescription>
                          </div>
                        </div>
                      </CardHeader>
                      
                      <CardContent className="flex flex-col gap-4">
                        <div className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
                          <span className="inline-flex items-center gap-1.5">
                            <Calendar />
                            Created {formatDate(app.created_at)}
                          </span>
                          {app.hostname && (
                            <span className="inline-flex min-w-0 items-center gap-1.5">
                              <ExternalLink />
                              <span className="truncate">{app.hostname}</span>
                            </span>
                          )}
                        </div>

                        {isRunning && appVm && (
                          <div className="flex flex-wrap items-center gap-2">
                            <Badge variant="secondary" className="gap-1.5">
                              <Cpu />
                              <span>{appVm.vcpus || 1} vCPU</span>
                            </Badge>
                            <Badge variant="secondary" className="gap-1.5">
                              <HardDrive />
                              <span>{appVm.memory_mib || 128} MB</span>
                            </Badge>
                          </div>
                        )}
                      </CardContent>
                    </Card>
                  </Link>
                );
              })
            )}
          </div>
        </div>

        {showCreateApp && <CreateAppModal onClose={() => setShowCreateApp(false)} />}
      </DashboardLayout>
    </AuthGuard>
  );
}
