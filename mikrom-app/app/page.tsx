"use client";

import { useState } from "react";
import Link from "next/link";
import { 
  Rocket, 
  Plus, 
  LayoutDashboard, 
  Activity, 
  ArrowRight,
  Container
} from "lucide-react";
import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { useVms } from "@/lib/hooks/use-vms";
import { useApps } from "@/lib/hooks/use-apps";
import { useHealth } from "@/lib/hooks/use-health";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, CardTitle, CardContent, CardDescription } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { CreateAppModal } from "@/components/CreateAppModal";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { cn } from "@/lib/utils";

export default function Page() {
  const { data: vms = [], isFetching: isFetchingVms } = useVms();
  const { data: apps = [], isLoading: isLoadingApps } = useApps();
  const { data: healthData, isError: isHealthError } = useHealth();
  const [showCreateApp, setShowCreateApp] = useState(false);

  const runningVms = vms.filter((v) => v.status.toLowerCase() === "running");
  const runningCount = runningVms.length;
  const pendingCount = vms.filter(
    (v) =>
      v.status.toLowerCase() === "scheduled" ||
      v.status.toLowerCase() === "pending" ||
      v.status.toLowerCase() === "building"
  ).length;

  // Map apps to their live status if available
  const appsWithStatus = apps.slice(0, 5).map(app => {
    const liveVm = vms.find(vm => vm.app_id === app.id || vm.app_name === app.name);
    return {
      ...app,
      status: liveVm ? liveVm.status : "Stopped"
    };
  });

  const isEmpty = !isLoadingApps && apps.length === 0;
  const hasUndeployedApps = apps.length > 0 && apps.every(app => !vms.some(vm => vm.app_id === app.id || vm.app_name === app.name));

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="space-y-8">
          {/* Header */}
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
            <div>
              <h1 className="text-3xl font-bold tracking-tight">
                Dashboard
              </h1>
              <p className="text-muted-foreground mt-1">
                Monitor and manage your cloud infrastructure.
              </p>
            </div>
            {!isEmpty && (
              <div className="flex items-center gap-3">
                <Button onClick={() => setShowCreateApp(true)}>
                  <Plus className="w-4 h-4 mr-2" />
                  New Application
                </Button>
              </div>
            )}
          </div>

          {hasUndeployedApps && (
            <Alert className="bg-primary/5 border-primary/20 shadow-sm">
              <Rocket className="h-4 w-4 text-primary" />
              <AlertTitle className="font-bold">Next Step: Deploy your first app</AlertTitle>
              <AlertDescription className="flex items-center justify-between flex-wrap gap-4 mt-1">
                <span>You have applications created but none are currently running in a microVM.</span>
                <Link href={`/apps/${apps[0].name}`}>
                  <Button size="sm" className="h-8">Deploy Now</Button>
                </Link>
              </AlertDescription>
            </Alert>
          )}

          {/* Stats Grid */}
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6">
            <Card className="relative overflow-hidden shadow-sm hover:shadow-md transition-shadow">
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">Total Applications</CardTitle>
                <Container className="w-4 h-4 text-muted-foreground" />
              </CardHeader>
              <CardContent>
                {isLoadingApps ? (
                  <div className="h-8 w-12 bg-muted animate-pulse rounded" />
                ) : (
                  <div className="text-2xl font-bold">{apps.length}</div>
                )}
                <p className="text-xs text-muted-foreground mt-1">
                  Active projects
                </p>
              </CardContent>
              <div className="absolute bottom-0 left-0 h-1 w-full bg-primary/10" />
            </Card>
            
            <Card className="relative overflow-hidden shadow-sm hover:shadow-md transition-shadow">
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">Running VMs</CardTitle>
                <Activity className="w-4 h-4 text-green-500" />
              </CardHeader>
              <CardContent>
                {isFetchingVms && vms.length === 0 ? (
                  <div className="h-8 w-12 bg-muted animate-pulse rounded" />
                ) : (
                  <div className="text-2xl font-bold text-green-600 dark:text-green-400">{runningCount}</div>
                )}
                <p className="text-xs text-muted-foreground mt-1">
                  Currently serving traffic
                </p>
              </CardContent>
              <div className="absolute bottom-0 left-0 h-1 w-full bg-green-500/10" />
            </Card>

            <Card className="relative overflow-hidden shadow-sm hover:shadow-md transition-shadow">
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">Deploying</CardTitle>
                <Rocket className="w-4 h-4 text-yellow-500" />
              </CardHeader>
              <CardContent>
                {isFetchingVms && vms.length === 0 ? (
                  <div className="h-8 w-12 bg-muted animate-pulse rounded" />
                ) : (
                  <div className="text-2xl font-bold text-yellow-600 dark:text-yellow-400">{pendingCount}</div>
                )}
                <p className="text-xs text-muted-foreground mt-1">
                  Ongoing deployments
                </p>
              </CardContent>
              <div className="absolute bottom-0 left-0 h-1 w-full bg-yellow-500/10" />
            </Card>
          </div>

          {isEmpty ? (
            /* Minimal Empty State */
            <div className="flex flex-col items-center justify-center min-h-[400px] border-2 border-dashed rounded-xl bg-muted/5 p-12 text-center">
              <div className="w-16 h-16 bg-primary/10 text-primary rounded-full flex items-center justify-center mb-6">
                <Rocket className="w-8 h-8" />
              </div>
              <h2 className="text-2xl font-bold tracking-tight">No applications found</h2>
              <p className="text-muted-foreground mt-2 max-w-sm">
                Get started by creating your first application and deploying it to Mikrom Cloud Platform.
              </p>
              <Button size="lg" className="mt-8 shadow-sm" onClick={() => setShowCreateApp(true)}>
                <Plus className="w-5 h-5 mr-2" />
                Create Application
              </Button>
            </div>
          ) : (
            /* Dashboard Content */
            <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
              <div className="lg:col-span-2 space-y-8">
                {/* Recent Applications */}
                <div className="space-y-4">
                  <div className="flex items-center justify-between">
                    <h2 className="text-xl font-semibold flex items-center gap-2">
                      <LayoutDashboard className="w-5 h-5 text-primary" />
                      Recent Applications
                    </h2>
                    <Link href="/apps">
                      <Button variant="ghost" size="sm" className="text-primary hover:bg-primary/5">
                        View all
                        <ArrowRight className="ml-2 h-4 w-4" />
                      </Button>
                    </Link>
                  </div>
                  
                  <Card className="overflow-hidden border-0 shadow-sm ring-1 ring-border">
                    <Table>
                      <TableHeader className="bg-muted/30">
                        <TableRow>
                          <TableHead className="py-3">Application</TableHead>
                          <TableHead className="py-3">Status</TableHead>
                          <TableHead className="py-3 text-right">Actions</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {isLoadingApps ? (
                          Array(3).fill(0).map((_, i) => (
                            <TableRow key={i}>
                              <TableCell><div className="h-4 w-32 bg-muted animate-pulse rounded" /></TableCell>
                              <TableCell><div className="h-4 w-16 bg-muted animate-pulse rounded" /></TableCell>
                              <TableCell className="text-right"><div className="h-8 w-20 bg-muted animate-pulse rounded ml-auto" /></TableCell>
                            </TableRow>
                          ))
                        ) : (
                          appsWithStatus.map((app) => (
                            <TableRow key={app.id} className="hover:bg-muted/10 transition-colors cursor-default">
                              <TableCell>
                                <div className="flex flex-col">
                                  <span className="font-bold text-base">{app.name}</span>
                                  <span className="text-xs text-muted-foreground truncate max-w-[250px]">{app.git_url}</span>
                                </div>
                              </TableCell>
                              <TableCell>
                                <Badge 
                                  variant={
                                    app.status.toLowerCase() === "running" ? "success" : 
                                    (app.status.toLowerCase() === "building" || app.status.toLowerCase() === "pending") ? "warning" : 
                                    "secondary"
                                  }
                                  className="capitalize px-3 py-1 font-medium"
                                >
                                  {app.status}
                                </Badge>
                              </TableCell>
                              <TableCell className="text-right">
                                <Link href={`/apps/${app.name}`}>
                                  <Button size="sm" variant="outline" className="h-8 px-4">
                                    Manage
                                  </Button>
                                </Link>
                              </TableCell>
                            </TableRow>
                          ))
                        )}
                      </TableBody>
                    </Table>
                  </Card>
                </div>
              </div>

              {/* Sidebar */}
              <div className="space-y-6">
                {/* System Health */}
                <Card className="shadow-sm overflow-hidden">
                  <CardHeader className="pb-3">
                    <CardTitle className="text-lg font-bold">System Status</CardTitle>
                    <CardDescription>Health of core services.</CardDescription>
                  </CardHeader>
                  <CardContent className="space-y-4">
                    {[
                      { name: "API", key: "API" },
                      { name: "Agents", key: "Agents" },
                      { name: "Scheduler", key: "Scheduler" },
                      { name: "Builder", key: "Builder" },
                      { name: "Router", key: "Router" },
                    ].map((service) => {
                      const status = healthData?.services?.[service.key] || (isHealthError ? 'OFFLINE' : 'CHECKING');
                      const isOnline = status === 'ONLINE';
                      const isChecking = status === 'CHECKING';
                      
                      return (
                        <div key={service.name} className="flex items-center justify-between">
                          <span className="text-sm font-medium">{service.name}</span>
                          <div className="flex items-center gap-2">
                            <span className="text-[10px] uppercase font-bold text-muted-foreground">{status}</span>
                            <div className={cn(
                              "w-2.5 h-2.5 rounded-full",
                              isOnline ? "bg-green-500" : (isChecking ? "bg-yellow-500 animate-pulse" : "bg-red-500 animate-pulse")
                            )} />
                          </div>
                        </div>
                      );
                    })}
                  </CardContent>
                  <div className="bg-muted/30 px-6 py-3 border-t">
                    <p className="text-[10px] text-muted-foreground uppercase font-bold tracking-widest">Version {healthData?.version || '0.0.0'}</p>
                  </div>
                </Card>
              </div>
            </div>
          )}
        </div>

        {showCreateApp && <CreateAppModal onClose={() => setShowCreateApp(false)} />}
      </DashboardLayout>
    </AuthGuard>
  );
}
