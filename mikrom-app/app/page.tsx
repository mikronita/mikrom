"use client";

import { useState } from "react";
import Link from "next/link";
import { 
  Rocket, 
  Plus, 
  Activity, 
  ArrowRight,
  Bot,
  CalendarClock,
  Container,
  Cpu,
  Hammer,
  Router,
  Server
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
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";

const formatDate = (dateStr: string) => {
  try {
    return new Intl.DateTimeFormat("en-US", {
      month: "short",
      day: "numeric",
      year: "numeric",
    }).format(new Date(dateStr));
  } catch {
    return dateStr;
  }
};

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
  const appsWithStatus = [...apps]
    .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
    .slice(0, 5)
    .map((app) => {
      const liveVm = vms.find(vm => vm.app_id === app.id || vm.app_name === app.name);
      return {
        ...app,
        liveVm,
        status: liveVm ? liveVm.status : "Stopped"
      };
    });

  const isEmpty = !isLoadingApps && apps.length === 0;
  const hasUndeployedApps = apps.length > 0 && apps.every(app => !vms.some(vm => vm.app_id === app.id || vm.app_name === app.name));
  const offlineServices = Object.values(healthData?.services ?? {}).filter((status) => status !== "ONLINE").length;
  const statCards = [
    {
      title: "Applications",
      value: apps.length,
      description: "Git projects in the workspace",
      icon: Container,
      loading: isLoadingApps,
    },
    {
      title: "Running VMs",
      value: runningCount,
      description: "Instances currently serving traffic",
      icon: Activity,
      loading: isFetchingVms && vms.length === 0,
    },
    {
      title: "Deploying",
      value: pendingCount,
      description: "Builds or starts in progress",
      icon: Rocket,
      loading: isFetchingVms && vms.length === 0,
    },
  ];

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="flex flex-col gap-8">
          <div className="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
            <div className="flex flex-col gap-2">
              <h1 className="text-3xl font-semibold tracking-tight">
                Dashboard
              </h1>
              <p className="max-w-2xl text-sm text-muted-foreground">
                Monitor and manage your cloud infrastructure.
              </p>
            </div>
            {!isEmpty && (
              <Button onClick={() => setShowCreateApp(true)}>
                <Plus data-icon="inline-start" />
                New Application
              </Button>
            )}
          </div>

          {hasUndeployedApps && (
            <Alert>
              <Rocket />
              <AlertTitle>Deploy your first app</AlertTitle>
              <AlertDescription className="flex flex-wrap items-center justify-between gap-4">
                <span>You have applications created but none are currently running in a microVM.</span>
                <Button size="sm" asChild>
                  <Link href={`/apps/${apps[0].name}`}>Deploy now</Link>
                </Button>
              </AlertDescription>
            </Alert>
          )}

          <div className="grid gap-4 md:grid-cols-3">
            {statCards.map((stat) => (
              <Card key={stat.title}>
                <CardHeader className="flex flex-row items-center justify-between gap-4 pb-2">
                  <CardTitle className="text-sm font-medium">{stat.title}</CardTitle>
                  <stat.icon className="text-muted-foreground" />
                </CardHeader>
                <CardContent className="flex flex-col gap-1">
                  {stat.loading ? (
                    <Skeleton className="h-8 w-16" />
                  ) : (
                    <div className="text-3xl font-semibold tracking-tight">{stat.value}</div>
                  )}
                  <p className="text-xs text-muted-foreground">{stat.description}</p>
                </CardContent>
              </Card>
            ))}
          </div>

          {isEmpty ? (
            <Empty className="min-h-[420px] border">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <Rocket />
                </EmptyMedia>
                <EmptyTitle>No applications found</EmptyTitle>
                <EmptyDescription>
                  Create your first application and deploy it to a Mikrom microVM.
                </EmptyDescription>
              </EmptyHeader>
              <EmptyContent>
                <Button onClick={() => setShowCreateApp(true)}>
                  <Plus data-icon="inline-start" />
                  Create Application
                </Button>
              </EmptyContent>
            </Empty>
          ) : (
            <div className="grid min-w-0 gap-6 lg:grid-cols-[minmax(0,1fr)_320px]">
              <Card className="min-w-0">
                <CardHeader className="border-b">
                  <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
                    <div className="flex items-start gap-3">
                      <div className="flex size-10 shrink-0 items-center justify-center rounded-md border bg-muted text-muted-foreground">
                        <Container />
                      </div>
                      <div className="flex flex-col gap-1.5">
                        <div className="flex flex-wrap items-center gap-2">
                          <CardTitle>Recent Applications</CardTitle>
                        </div>
                        <CardDescription>Latest projects and their runtime state.</CardDescription>
                      </div>
                    </div>
                    <Button variant="outline" size="sm" asChild>
                      <Link href="/apps">
                        View all
                        <ArrowRight data-icon="inline-end" />
                      </Link>
                    </Button>
                  </div>
                </CardHeader>
                <CardContent className="p-0">
                  <Table className="table-fixed">
                    <TableHeader>
                      <TableRow>
                        <TableHead className="w-[52%] pl-6">Application</TableHead>
                        <TableHead className="w-[24%]">Status</TableHead>
                        <TableHead className="hidden w-[24%] xl:table-cell">Created</TableHead>
                        <TableHead className="w-[96px] pr-6 text-right">Actions</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {isLoadingApps ? (
                        Array.from({ length: 3 }).map((_, i) => (
                          <TableRow key={i}>
                            <TableCell className="pl-6"><Skeleton className="h-9 w-44" /></TableCell>
                            <TableCell><Skeleton className="h-5 w-20" /></TableCell>
                            <TableCell className="hidden xl:table-cell"><Skeleton className="h-5 w-24" /></TableCell>
                            <TableCell className="pr-6 text-right"><Skeleton className="ml-auto h-8 w-20" /></TableCell>
                          </TableRow>
                        ))
                      ) : (
                        appsWithStatus.map((app) => (
                          <TableRow key={app.id}>
                            <TableCell className="pl-6">
                              <div className="flex min-w-0 items-center gap-3">
                                <div className="flex size-9 shrink-0 items-center justify-center rounded-md border bg-muted text-muted-foreground">
                                  <Server />
                                </div>
                                <div className="flex min-w-0 flex-col gap-1">
                                  <span className="truncate font-medium">{app.name}</span>
                                  <span className="truncate text-xs text-muted-foreground">
                                    {app.hostname || "No public hostname"}
                                  </span>
                                </div>
                              </div>
                            </TableCell>
                            <TableCell>
                              <Badge
                                variant={
                                  app.status.toLowerCase() === "running" ? "success" :
                                  (app.status.toLowerCase() === "building" || app.status.toLowerCase() === "pending") ? "warning" :
                                  "secondary"
                                }
                                className="capitalize"
                              >
                                {app.status}
                              </Badge>
                            </TableCell>
                            <TableCell className="hidden text-sm text-muted-foreground xl:table-cell">
                              {formatDate(app.created_at)}
                            </TableCell>
                            <TableCell className="pr-6 text-right">
                              <Button size="sm" variant="outline" asChild>
                                <Link href={`/apps/${app.name}`}>Manage</Link>
                              </Button>
                            </TableCell>
                          </TableRow>
                        ))
                      )}
                    </TableBody>
                  </Table>
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <div className="flex items-center justify-between gap-4">
                    <div className="flex flex-col gap-1.5">
                      <CardTitle>System Status</CardTitle>
                      <CardDescription>Health of core services.</CardDescription>
                    </div>
                    <Badge variant={isHealthError || offlineServices > 0 ? "destructive" : "secondary"}>
                      {isHealthError || offlineServices > 0 ? "Degraded" : "Operational"}
                    </Badge>
                  </div>
                </CardHeader>
                <CardContent className="flex flex-col gap-4">
                  {[
                    { name: "API", key: "API", icon: Cpu },
                    { name: "Agents", key: "Agents", icon: Bot },
                    { name: "Scheduler", key: "Scheduler", icon: CalendarClock },
                    { name: "Builder", key: "Builder", icon: Hammer },
                    { name: "Router", key: "Router", icon: Router },
                  ].map((service, index, array) => {
                    const status = healthData?.services?.[service.key] || (isHealthError ? "OFFLINE" : "CHECKING");
                    const isOnline = status === "ONLINE";
                    const isChecking = status === "CHECKING";
                    const ServiceIcon = service.icon;

                    return (
                      <div key={service.name} className="flex flex-col gap-4">
                        <div className="flex items-center justify-between gap-4">
                          <div className="flex items-center gap-2">
                            <ServiceIcon className="text-muted-foreground" />
                            <span className="text-sm font-medium">{service.name}</span>
                          </div>
                          <Badge
                            variant={isOnline ? "secondary" : isChecking ? "outline" : "destructive"}
                            className="uppercase"
                          >
                            {status}
                          </Badge>
                        </div>
                        {index < array.length - 1 && <Separator />}
                      </div>
                    );
                  })}
                </CardContent>
                <CardContent className="border-t pt-4">
                  <p className="text-xs text-muted-foreground">Version {healthData?.version || "0.0.0"}</p>
                </CardContent>
              </Card>
            </div>
          )}
        </div>

        {showCreateApp && <CreateAppModal onClose={() => setShowCreateApp(false)} />}
      </DashboardLayout>
    </AuthGuard>
  );
}
