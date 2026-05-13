"use client";

import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Boxes,
  Globe2,
  Loader2,
  LockKeyhole,
  Network,
  Plus,
  Server,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";

import Link from "next/link";

import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  CreateSecurityRuleRequest,
  createSecurityRule,
  deleteSecurityRule,
  getMeshStatus,
  getUserProfile,
  listActiveDeployments,
  listApps,
  listSecurityRules,
  watchMeshStatus,
} from "@/lib/api";
import { getToken } from "@/lib/auth";

const defaultRule: CreateSecurityRuleRequest = {
  protocol: "tcp",
  port_start: 80,
  port_end: 80,
  action: "allow",
};

function formatPortRange(rule: { protocol: string; port_start: number; port_end: number }) {
  if (rule.protocol === "any") return "All ports";
  if (rule.port_start === rule.port_end) return rule.port_start;
  return `${rule.port_start}-${rule.port_end}`;
}

function formatVmId(vmId: string) {
  return vmId.length > 12 ? vmId.substring(0, 12) : vmId;
}

export default function NetworkingPage() {
  const token = getToken();
  const queryClient = useQueryClient();
  const [selectedApp, setSelectedApp] = useState<string | null>(null);
  const [isAddRuleOpen, setIsAddRuleOpen] = useState(false);
  const [newRule, setNewRule] = useState<CreateSecurityRuleRequest>(defaultRule);

  const { data: profile, isLoading: profileLoading } = useQuery({
    queryKey: ["profile"],
    queryFn: () =>
      getUserProfile(token!).then((res) => {
        if (res.error) throw new Error(res.error);
        return res.data;
      }),
    enabled: !!token,
  });

  const { data: deployments, isLoading: deploymentsLoading } = useQuery({
    queryKey: ["active-deployments"],
    queryFn: () =>
      listActiveDeployments(token!).then((res) => {
        if (res.error) throw new Error(res.error);
        return res.data;
      }),
    enabled: !!token,
  });

  const { data: mesh, isLoading: meshLoading } = useQuery({
    queryKey: ["mesh-status"],
    queryFn: () =>
      getMeshStatus(token!).then((res) => {
        if (res.error) throw new Error(res.error);
        return res.data;
      }),
    enabled: !!token,
  });

  useEffect(() => {
    if (!token) return;

    const cleanup = watchMeshStatus(token, (data) => {
      queryClient.setQueryData(["mesh-status"], data);
    });

    return () => cleanup();
  }, [queryClient, token]);

  const { data: apps } = useQuery({
    queryKey: ["apps"],
    queryFn: () =>
      listApps(token!).then((res) => {
        if (res.error) throw new Error(res.error);
        return res.data;
      }),
    enabled: !!token,
  });

  const { data: rules, isLoading: rulesLoading } = useQuery({
    queryKey: ["security-rules", selectedApp],
    queryFn: () =>
      listSecurityRules(token!, selectedApp!).then((res) => {
        if (res.error) throw new Error(res.error);
        return res.data;
      }),
    enabled: !!token && !!selectedApp,
  });

  const runningDeployments = deployments?.filter((deployment) => deployment.status === "RUNNING") || [];

  const createRuleMutation = useMutation({
    mutationFn: (data: CreateSecurityRuleRequest) => createSecurityRule(token!, selectedApp!, data),
    onSuccess: (res) => {
      if (res.error) {
        toast.error(res.error);
        return;
      }

      toast.success("Security rule created");
      queryClient.invalidateQueries({ queryKey: ["security-rules", selectedApp] });
      setNewRule(defaultRule);
      setIsAddRuleOpen(false);
    },
  });

  const deleteRuleMutation = useMutation({
    mutationFn: (ruleId: string) => deleteSecurityRule(token!, selectedApp!, ruleId),
    onSuccess: (res) => {
      if (res.error) {
        toast.error(res.error);
        return;
      }

      toast.success("Security rule deleted");
      queryClient.invalidateQueries({ queryKey: ["security-rules", selectedApp] });
    },
  });

  const summaryCards = [
    {
      label: "VPC prefix",
      value: profile?.vpc_ipv6_prefix || "Not assigned",
      description: "Private IPv6 /40 prefix reserved for your applications.",
      icon: Globe2,
      loading: profileLoading,
      valueClassName: "break-all font-mono text-lg",
    },
    {
      label: "Active peers",
      value: mesh?.total_workers ?? 0,
      description: "Agent nodes currently participating in the mesh.",
      icon: Server,
      loading: meshLoading,
      valueClassName: "text-3xl",
    },
    {
      label: "Running workloads",
      value: runningDeployments.length,
      description: "MicroVMs currently reachable through 6PN.",
      icon: Boxes,
      loading: deploymentsLoading,
      valueClassName: "text-3xl",
    },
  ];

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="flex flex-col gap-6">
          <div className="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
            <div className="flex flex-col gap-2">
              <div className="flex items-center gap-3">
                <div className="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
                  <Network />
                </div>
                <h1 className="text-3xl font-semibold tracking-tight">Networking</h1>
              </div>
              <p className="max-w-2xl text-sm text-muted-foreground">
                Monitor the private 6PN mesh, workload addresses and application security rules.
              </p>
            </div>
            <Badge variant="secondary" className="w-fit gap-2 px-3 py-1.5">
              <LockKeyhole />
              WireGuard mesh
            </Badge>
          </div>

          <div className="grid gap-4 md:grid-cols-3">
            {summaryCards.map((item) => (
              <Card key={item.label}>
                <CardHeader className="flex flex-row items-start justify-between gap-4 pb-3">
                  <div className="flex flex-col gap-1">
                    <CardDescription>{item.label}</CardDescription>
                    {item.loading ? (
                      <Skeleton className="mt-1 h-8 w-32" />
                    ) : (
                      <CardTitle className={item.valueClassName}>{item.value}</CardTitle>
                    )}
                  </div>
                  <div className="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
                    <item.icon />
                  </div>
                </CardHeader>
                <CardContent>
                  <p className="text-sm text-muted-foreground">{item.description}</p>
                </CardContent>
              </Card>
            ))}
          </div>

          <div className="grid gap-4 xl:grid-cols-[minmax(0,1.15fr)_minmax(24rem,0.85fr)]">
            <Card className="overflow-hidden">
              <CardHeader className="border-b bg-muted/20">
                <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
                  <div className="flex flex-col gap-1.5">
                    <CardTitle className="flex items-center gap-2 text-lg">
                      <Boxes />
                      Workload connectivity
                    </CardTitle>
                    <CardDescription>
                      Running microVMs reachable inside your private 6PN mesh.
                    </CardDescription>
                  </div>
                  <Badge variant="secondary" className="w-fit gap-2">
                    <Network />
                    {runningDeployments.length} active
                  </Badge>
                </div>
              </CardHeader>
              <CardContent className="p-0">
                <Table>
                  <TableHeader>
                    <TableRow className="hover:bg-transparent">
                      <TableHead className="px-6">Workload</TableHead>
                      <TableHead>6PN address</TableHead>
                      <TableHead className="pr-6 text-right">Health</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {deploymentsLoading ? (
                      Array.from({ length: 3 }).map((_, index) => (
                        <TableRow key={index}>
                          <TableCell colSpan={3} className="p-4">
                            <Skeleton className="h-10 w-full" />
                          </TableCell>
                        </TableRow>
                      ))
                    ) : runningDeployments.length === 0 ? (
                      <TableRow>
                        <TableCell colSpan={3} className="p-0">
                          <Empty className="border-0 py-14">
                            <EmptyHeader>
                              <EmptyMedia variant="icon">
                                <Network />
                              </EmptyMedia>
                              <EmptyTitle>No active workloads</EmptyTitle>
                              <EmptyDescription>
                                Running deployments will appear here with their private network address.
                              </EmptyDescription>
                            </EmptyHeader>
                          </Empty>
                        </TableCell>
                      </TableRow>
                    ) : (
                      runningDeployments.map((deployment) => (
                        <TableRow key={deployment.vm_id}>
                          <TableCell className="px-6">
                            <Link
                              href={`/apps/${encodeURIComponent(deployment.app_name)}`}
                              className="flex items-center gap-3 hover:opacity-80"
                            >
                              <div className="flex size-9 shrink-0 items-center justify-center rounded-md border bg-background text-muted-foreground">
                                <Boxes />
                              </div>
                              <div className="min-w-0">
                                <div className="truncate font-medium">{deployment.app_name}</div>
                                <div className="font-mono text-xs text-muted-foreground">
                                  vm-{formatVmId(deployment.vm_id)}
                                </div>
                              </div>
                            </Link>
                          </TableCell>
                          <TableCell>
                            <div className="flex flex-col gap-1">
                              <span className="w-fit rounded-md border bg-muted/40 px-2 py-1 font-mono text-xs">
                                {deployment.ipv6_address || "Assigning address..."}
                              </span>
                              <span className="text-xs text-muted-foreground">Private mesh endpoint</span>
                            </div>
                          </TableCell>
                          <TableCell className="pr-6 text-right">
                            <Badge variant="success" className="capitalize">
                              {deployment.status.toLowerCase()}
                            </Badge>
                          </TableCell>
                        </TableRow>
                      ))
                    )}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>

            <Card className="overflow-hidden">
              <CardHeader className="border-b bg-muted/20">
                <div className="flex flex-col gap-4">
                  <div className="flex flex-col gap-1.5">
                    <CardTitle className="flex items-center gap-2 text-lg">
                      <ShieldCheck />
                      Security groups
                    </CardTitle>
                    <CardDescription>
                      L3/L4 rules applied to every active microVM for an application.
                    </CardDescription>
                  </div>
                  <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
                    <Select onValueChange={(value) => setSelectedApp(value)}>
                      <SelectTrigger className="sm:w-[220px]">
                        <SelectValue placeholder="Select application" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectGroup>
                          {apps?.map((app) => (
                            <SelectItem key={app.id} value={app.name}>
                              {app.name}
                            </SelectItem>
                          ))}
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                    {selectedApp && (
                      <Dialog open={isAddRuleOpen} onOpenChange={setIsAddRuleOpen}>
                        <DialogTrigger asChild>
                          <Button size="sm">
                            <Plus data-icon="inline-start" />
                            Add rule
                          </Button>
                        </DialogTrigger>
                        <DialogContent>
                          <DialogHeader>
                            <DialogTitle>Add security rule</DialogTitle>
                            <DialogDescription>
                              Create a firewall rule for <strong>{selectedApp}</strong>.
                            </DialogDescription>
                          </DialogHeader>
                          <FieldGroup>
                            <Field>
                              <FieldLabel htmlFor="protocol">Protocol</FieldLabel>
                              <Select
                                value={newRule.protocol}
                                onValueChange={(value) => setNewRule({ ...newRule, protocol: value })}
                              >
                                <SelectTrigger id="protocol">
                                  <SelectValue />
                                </SelectTrigger>
                                <SelectContent>
                                  <SelectGroup>
                                    <SelectItem value="tcp">TCP</SelectItem>
                                    <SelectItem value="udp">UDP</SelectItem>
                                    <SelectItem value="any">Any</SelectItem>
                                  </SelectGroup>
                                </SelectContent>
                              </Select>
                            </Field>
                            <div className="grid gap-4 sm:grid-cols-2">
                              <Field>
                                <FieldLabel htmlFor="port_start">Port start</FieldLabel>
                                <Input
                                  id="port_start"
                                  type="number"
                                  min={0}
                                  max={65535}
                                  value={newRule.port_start}
                                  disabled={newRule.protocol === "any"}
                                  onChange={(event) =>
                                    setNewRule({
                                      ...newRule,
                                      port_start: Number.parseInt(event.target.value, 10) || 0,
                                    })
                                  }
                                />
                              </Field>
                              <Field>
                                <FieldLabel htmlFor="port_end">Port end</FieldLabel>
                                <Input
                                  id="port_end"
                                  type="number"
                                  min={0}
                                  max={65535}
                                  value={newRule.port_end}
                                  disabled={newRule.protocol === "any"}
                                  onChange={(event) =>
                                    setNewRule({
                                      ...newRule,
                                      port_end: Number.parseInt(event.target.value, 10) || 0,
                                    })
                                  }
                                />
                              </Field>
                            </div>
                            <Field>
                              <FieldLabel htmlFor="action">Action</FieldLabel>
                              <Select
                                value={newRule.action}
                                onValueChange={(value) => setNewRule({ ...newRule, action: value })}
                              >
                                <SelectTrigger id="action">
                                  <SelectValue />
                                </SelectTrigger>
                                <SelectContent>
                                  <SelectGroup>
                                    <SelectItem value="allow">Allow</SelectItem>
                                    <SelectItem value="deny">Deny</SelectItem>
                                  </SelectGroup>
                                </SelectContent>
                              </Select>
                              <FieldDescription>
                                Rules are evaluated by the control plane and distributed to active workers.
                              </FieldDescription>
                            </Field>
                          </FieldGroup>
                          <DialogFooter>
                            <Button
                              onClick={() => createRuleMutation.mutate(newRule)}
                              disabled={createRuleMutation.isPending}
                            >
                              {createRuleMutation.isPending && (
                                <Loader2 data-icon="inline-start" className="animate-spin" />
                              )}
                              Create rule
                            </Button>
                          </DialogFooter>
                        </DialogContent>
                      </Dialog>
                    )}
                  </div>
                </div>
              </CardHeader>
              <CardContent className="p-0">
                {!selectedApp ? (
                  <Empty className="border-0 py-14">
                    <EmptyHeader>
                      <EmptyMedia variant="icon">
                        <ShieldCheck />
                      </EmptyMedia>
                      <EmptyTitle>Select an application</EmptyTitle>
                      <EmptyDescription>
                        Choose an app to inspect and manage its security group rules.
                      </EmptyDescription>
                    </EmptyHeader>
                  </Empty>
                ) : rulesLoading ? (
                  <div className="flex flex-col gap-3 p-6">
                    <Skeleton className="h-10 w-full" />
                    <Skeleton className="h-10 w-full" />
                    <Skeleton className="h-10 w-full" />
                  </div>
                ) : rules?.length === 0 ? (
                  <Empty className="border-0 py-14">
                    <EmptyHeader>
                      <EmptyMedia variant="icon">
                        <ShieldCheck />
                      </EmptyMedia>
                      <EmptyTitle>No rules defined</EmptyTitle>
                      <EmptyDescription>
                        By default, internal 6PN traffic remains open for this application.
                      </EmptyDescription>
                    </EmptyHeader>
                    <EmptyContent>
                      <Button size="sm" onClick={() => setIsAddRuleOpen(true)}>
                        <Plus data-icon="inline-start" />
                        Add first rule
                      </Button>
                    </EmptyContent>
                  </Empty>
                ) : (
                  <Table>
                    <TableHeader>
                      <TableRow className="hover:bg-transparent">
                        <TableHead className="px-6">Protocol</TableHead>
                        <TableHead>Ports</TableHead>
                        <TableHead>Action</TableHead>
                        <TableHead className="pr-6 text-right">Actions</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {rules?.map((rule) => (
                        <TableRow key={rule.id}>
                          <TableCell className="px-6 font-medium uppercase">{rule.protocol}</TableCell>
                          <TableCell className="font-mono text-xs text-muted-foreground">
                            {formatPortRange(rule)}
                          </TableCell>
                          <TableCell>
                            <Badge variant={rule.action === "allow" ? "success" : "destructive"}>
                              {rule.action}
                            </Badge>
                          </TableCell>
                          <TableCell className="pr-6 text-right">
                            <Button
                              variant="ghost"
                              size="icon"
                              className="text-destructive hover:bg-destructive/10"
                              onClick={() => deleteRuleMutation.mutate(rule.id)}
                              disabled={deleteRuleMutation.isPending}
                            >
                              <Trash2 />
                              <span className="sr-only">Delete rule</span>
                            </Button>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                )}
              </CardContent>
            </Card>
          </div>
        </div>
      </DashboardLayout>
    </AuthGuard>
  );
}
