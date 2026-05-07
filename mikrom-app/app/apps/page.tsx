"use client";

import { useState } from "react";
import Link from "next/link";
import { 
  HiPlus, 
  HiCollection, 
  HiExclamationCircle,
  HiExternalLink,
  HiCalendar,
  HiChip,
  HiDatabase
} from "react-icons/hi";
import { FolderPlus } from "lucide-react";
import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { useApps } from "@/lib/hooks/use-apps";
import { useVms, useWatchVms } from "@/lib/hooks/use-vms";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
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
import { cn } from "@/lib/utils";
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

          {(appsError || vmsError) && (
            <Alert variant="destructive">
              <HiExclamationCircle className="h-4 w-4" />
              <AlertDescription>
                {appsError?.message || vmsError?.message || "Failed to load applications"}
              </AlertDescription>
            </Alert>
          )}

          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
            {isLoadingApps && apps.length === 0 ? (
              Array.from({ length: 6 }).map((_, i) => (
                <Card key={i} className="overflow-hidden">
                  <CardHeader className="p-6">
                    <div className="flex items-center gap-4">
                      <div className="w-10 h-10 rounded-lg bg-muted animate-pulse" />
                      <div className="space-y-2">
                        <div className="h-4 w-24 bg-muted animate-pulse rounded" />
                        <div className="h-3 w-32 bg-muted animate-pulse rounded" />
                      </div>
                    </div>
                  </CardHeader>
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
                      <HiPlus className="w-4 h-4 mr-2" />
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
                    className="block group"
                  >
                    <Card 
                      className="h-full hover:border-primary/50 transition-all duration-200 flex flex-col shadow-sm cursor-pointer group-hover:shadow-md group-hover:-translate-y-1"
                    >
                      <CardHeader className="p-6 pb-4">
                        <div className="flex items-start gap-4">
                          <div className={cn(
                            "w-12 h-12 rounded-xl flex items-center justify-center border bg-card shadow-sm transition-all group-hover:scale-110 shrink-0",
                            isRunning ? "text-green-500 border-green-500/20 bg-green-500/5" : "text-muted-foreground bg-muted/20"
                          )}>
                            <HiCollection className="w-6 h-6" />
                          </div>
                          <div className="min-w-0 flex-1">
                            <CardTitle className="text-xl font-bold flex items-center gap-2">
                              <span className="truncate">{app.name}</span>
                              {isRunning && <Badge variant="success" className="text-[10px] py-0 px-1.5 h-4 uppercase font-bold tracking-wider">Live</Badge>}
                            </CardTitle>
                            <div className="text-[10px] text-muted-foreground mt-2 bg-muted/30 px-2 py-1.5 rounded-md border border-dashed leading-relaxed group-hover:bg-muted/50 transition-colors">
                              <span className="font-mono break-all">
                                {app.git_url}
                              </span>
                            </div>
                          </div>
                        </div>
                      </CardHeader>
                      
                      <CardContent className="px-6 pb-6 pt-2 space-y-4">
                        <div className="flex items-center gap-2 text-xs text-muted-foreground">
                          <HiCalendar className="w-3.5 h-3.5" />
                          <span>Created {formatDate(app.created_at)}</span>
                        </div>
                        
                        {app.hostname && (
                          <div className="flex items-center gap-2 text-xs">
                            <HiExternalLink className="w-3.5 h-3.5 text-indigo-500" />
                            <span className="text-indigo-500 hover:underline truncate">
                              {app.hostname}
                            </span>
                          </div>
                        )}

                        {isRunning && appVm && (
                          <div className="flex items-center gap-4 pt-1">
                            <div className="flex items-center gap-1.5 text-xs font-medium">
                              <HiChip className="w-3.5 h-3.5 text-orange-500" />
                              <span>{appVm.vcpus || 1} vCPU</span>
                            </div>
                            <div className="flex items-center gap-1.5 text-xs font-medium">
                              <HiDatabase className="w-3.5 h-3.5 text-blue-500" />
                              <span>{appVm.memory_mib || 128} MB</span>
                            </div>
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
