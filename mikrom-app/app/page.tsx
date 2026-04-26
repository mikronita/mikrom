"use client";

import { useState } from "react";
import Link from "next/link";
import { 
  Rocket, 
  Plus, 
  LayoutDashboard, 
  Activity, 
  BookOpen,
  Settings,
  ArrowRight,
  Container,
  Cpu,
  Zap,
  CheckCircle2,
  Clock
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
import { Progress } from "@/components/ui/progress";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { cn } from "@/lib/utils";

export default function Page() {
  const { data: vms = [], isFetching: isFetchingVms, error: vmsError } = useVms();
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

  // Resource Calculations
  const totalVcpus = runningVms.reduce((acc, vm) => acc + (vm.vcpus || 0), 0);
  const totalMemory = runningVms.reduce((acc, vm) => acc + (vm.memory_mib || 0), 0);
  
  // Assuming a dev cluster limit for visualization
  const MAX_VCPUS = 8;
  const MAX_MEMORY = 16384; // 16GB

  const vcpuProgress = Math.min((totalVcpus / MAX_VCPUS) * 100, 100);
  const memoryProgress = Math.min((totalMemory / MAX_MEMORY) * 100, 100);

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
            /* Onboarding / Empty State */
            <div className="mt-12">
              <Empty className="border-2 border-dashed bg-muted/20">
                <EmptyHeader>
                  <EmptyMedia variant="icon" className="bg-primary/10 text-primary scale-125 mb-4">
                    <Rocket className="w-8 h-8" />
                  </EmptyMedia>
                  <EmptyTitle className="text-3xl">Welcome to Mikrom</EmptyTitle>
                  <EmptyDescription className="text-lg max-w-lg mx-auto">
                    Mikrom allows you to deploy containerized workloads into high-performance Firecracker microVMs in seconds.
                  </EmptyDescription>
                </EmptyHeader>
                
                <div className="grid grid-cols-1 md:grid-cols-3 gap-8 w-full max-w-4xl mt-12 mb-8">
                  <div className="flex flex-col items-center text-center space-y-3 p-6 rounded-xl bg-background/50 border shadow-sm">
                    <div className="w-12 h-12 rounded-full bg-primary/10 text-primary flex items-center justify-center font-bold text-lg">1</div>
                    <h4 className="font-bold">Connect Git</h4>
                    <p className="text-sm text-muted-foreground leading-relaxed">Link your GitHub repository to Mikrom and we'll detect your app type automatically.</p>
                  </div>
                  <div className="flex flex-col items-center text-center space-y-3 p-6 rounded-xl bg-background/50 border shadow-sm">
                    <div className="w-12 h-12 rounded-full bg-primary/10 text-primary flex items-center justify-center font-bold text-lg">2</div>
                    <h4 className="font-bold">Configure</h4>
                    <p className="text-sm text-muted-foreground leading-relaxed">Define vCPUs, RAM and environment variables to match your application's needs.</p>
                  </div>
                  <div className="flex flex-col items-center text-center space-y-3 p-6 rounded-xl bg-background/50 border shadow-sm">
                    <div className="w-12 h-12 rounded-full bg-primary/10 text-primary flex items-center justify-center font-bold text-lg">3</div>
                    <h4 className="font-bold">Deploy</h4>
                    <p className="text-sm text-muted-foreground leading-relaxed">Your app will be built and deployed to a dedicated Firecracker VM instantly.</p>
                  </div>
                </div>

                <EmptyContent>
                  <Button size="lg" className="mt-4 px-10 h-12 text-lg shadow-lg" onClick={() => setShowCreateApp(true)}>
                    <Plus className="w-6 h-6 mr-2" />
                    Create Your First Application
                  </Button>
                </EmptyContent>
              </Empty>
            </div>
          ) : (
            /* Dashboard Content */
            <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
              <div className="lg:col-span-2 space-y-8">
                {/* Resource Utilization */}
                <Card className="shadow-sm">
                  <CardHeader className="pb-4">
                    <CardTitle className="text-lg font-bold flex items-center gap-2">
                      <Cpu className="w-5 h-5 text-primary" />
                      Cluster Resources
                    </CardTitle>
                    <CardDescription>Aggregate usage across all running MicroVMs.</CardDescription>
                  </CardHeader>
                  <CardContent className="space-y-6">
                    <div className="space-y-2">
                      <div className="flex items-center justify-between text-sm">
                        <span className="font-medium flex items-center gap-2">
                          <Cpu className="w-4 h-4 text-muted-foreground" />
                          vCPU Allocation
                        </span>
                        <span className="text-muted-foreground font-mono">{totalVcpus} / {MAX_VCPUS} Cores</span>
                      </div>
                      <Progress value={vcpuProgress} className="h-2 bg-muted" />
                    </div>
                    <div className="space-y-2">
                      <div className="flex items-center justify-between text-sm">
                        <span className="font-medium flex items-center gap-2">
                          <Zap className="w-4 h-4 text-muted-foreground" />
                          RAM Allocation
                        </span>
                        <span className="text-muted-foreground font-mono">{(totalMemory / 1024).toFixed(1)}GB / {MAX_MEMORY / 1024}GB</span>
                      </div>
                      <Progress value={memoryProgress} className="h-2 bg-muted" />
                    </div>
                  </CardContent>
                </Card>

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
                    <div className="flex items-center justify-between">
                      <span className="text-sm font-medium">API Service</span>
                      <div className="flex items-center gap-2">
                        <span className="text-[10px] uppercase font-bold text-muted-foreground">{healthData?.status || 'checking'}</span>
                        <div className={cn("w-2.5 h-2.5 rounded-full", isHealthError ? "bg-red-500 animate-pulse" : "bg-green-500")} />
                      </div>
                    </div>
                    <div className="flex items-center justify-between">
                      <span className="text-sm font-medium">Scheduler</span>
                      <div className="flex items-center gap-2">
                        <span className="text-[10px] uppercase font-bold text-muted-foreground">{vmsError ? 'error' : 'online'}</span>
                        <div className={cn("w-2.5 h-2.5 rounded-full", vmsError ? "bg-red-500 animate-pulse" : "bg-green-500")} />
                      </div>
                    </div>
                    <div className="flex items-center justify-between">
                      <span className="text-sm font-medium">Build Engine</span>
                      <div className="flex items-center gap-2">
                        <span className="text-[10px] uppercase font-bold text-muted-foreground">{healthData ? 'ready' : 'checking'}</span>
                        <div className={cn("w-2.5 h-2.5 rounded-full", !healthData ? "bg-yellow-500 animate-pulse" : "bg-green-500")} />
                      </div>
                    </div>
                  </CardContent>
                  <div className="bg-muted/30 px-6 py-3 border-t">
                    <p className="text-[10px] text-muted-foreground uppercase font-bold tracking-widest">Version {healthData?.version || '0.0.0'}</p>
                  </div>
                </Card>

                {/* Quick Actions */}
                <Card className="shadow-sm">
                  <CardHeader className="pb-3">
                    <CardTitle className="text-lg font-bold">Quick Actions</CardTitle>
                  </CardHeader>
                  <CardContent className="grid gap-4">
                    <Button variant="outline" className="group justify-start h-auto py-3 px-4 border-2 hover:border-primary/50 hover:bg-primary/5 transition-all" onClick={() => setShowCreateApp(true)}>
                      <Plus className="mr-3 h-5 w-5 text-primary group-hover:scale-110 transition-transform" />
                      <div className="flex flex-col items-start">
                        <span className="font-bold text-sm">New App</span>
                      </div>
                    </Button>
                    <Button variant="outline" className="group justify-start h-auto py-3 px-4 hover:bg-blue-50 dark:hover:bg-blue-900/10 transition-all" asChild>
                      <Link href="/docs" className="flex">
                        <BookOpen className="mr-3 h-5 w-5 text-blue-500 group-hover:scale-110 transition-transform" />
                        <div className="flex flex-col items-start">
                          <span className="font-bold text-sm">Documentation</span>
                        </div>
                      </Link>
                    </Button>
                    <Button variant="outline" className="group justify-start h-auto py-3 px-4 hover:bg-muted transition-all" asChild>
                      <Link href="/settings" className="flex">
                        <Settings className="mr-3 h-5 w-5 text-muted-foreground group-hover:scale-110 transition-transform" />
                        <div className="flex flex-col items-start">
                          <span className="font-bold text-sm">Settings</span>
                        </div>
                      </Link>
                    </Button>
                  </CardContent>
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
