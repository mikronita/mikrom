"use client";

import React, { useState } from "react";
import { DashboardLayout } from "@/components/DashboardLayout";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { 
  getUserProfile, 
  listActiveDeployments, 
  getMeshStatus, 
  listApps, 
  listSecurityRules, 
  createSecurityRule, 
  deleteSecurityRule,
  CreateSecurityRuleRequest
} from "@/lib/api";
import { getToken } from "@/lib/auth";
import {
  HiServer, 
  HiOutlineGlobeAlt,
  HiOutlineShieldCheck,
  HiOutlineCube,
  HiPlus,
  HiTrash,
  HiInformationCircle
} from "react-icons/hi";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { 
  Table, 
  TableBody, 
  TableCell, 
  TableHead, 
  TableHeader, 
  TableRow 
} from "@/components/ui/table";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Network } from "lucide-react";

export default function NetworkingPage() {
  const token = getToken();
  const queryClient = useQueryClient();
  const [selectedApp, setSelectedApp] = useState<string | null>(null);
  const [isAddRuleOpen, setIsAddRuleOpen] = useState(false);
  const [mounted, setMounted] = useState(false);

  React.useEffect(() => {
    setMounted(true);
  }, []);

  // Form state
  const [newRule, setNewRule] = useState<CreateSecurityRuleRequest>({
    protocol: "tcp",
    port_start: 80,
    port_end: 80,
    action: "allow"
  });

  const { data: profile, isLoading: profileLoading } = useQuery({
    queryKey: ["profile"],
    queryFn: () => getUserProfile(token!).then(res => {
      if (res.error) throw new Error(res.error);
      return res.data;
    }),
    enabled: !!token && mounted,
  });

  const { data: deployments, isLoading: deploymentsLoading } = useQuery({
    queryKey: ["active-deployments"],
    queryFn: () => listActiveDeployments(token!).then(res => {
      if (res.error) throw new Error(res.error);
      return res.data;
    }),
    enabled: !!token && mounted,
    refetchInterval: 5000,
  });

  const { data: mesh, isLoading: meshLoading } = useQuery({
    queryKey: ["mesh-status"],
    queryFn: () => getMeshStatus(token!).then(res => {
      if (res.error) throw new Error(res.error);
      return res.data;
    }),
    enabled: !!token && mounted,
    refetchInterval: 10000,
  });

  const { data: apps } = useQuery({
    queryKey: ["apps"],
    queryFn: () => listApps(token!).then(res => {
      if (res.error) throw new Error(res.error);
      return res.data;
    }),
    enabled: !!token && mounted,
  });

  const { data: rules, isLoading: rulesLoading } = useQuery({
    queryKey: ["security-rules", selectedApp],
    queryFn: () => listSecurityRules(token!, selectedApp!).then(res => {
      if (res.error) throw new Error(res.error);
      return res.data;
    }),
    enabled: !!token && !!selectedApp && mounted,
  });

  const createRuleMutation = useMutation({
    mutationFn: (data: CreateSecurityRuleRequest) => createSecurityRule(token!, selectedApp!, data),
    onSuccess: (res) => {
      if (res.error) {
        toast.error(res.error);
      } else {
        toast.success("Security rule created");
        queryClient.invalidateQueries({ queryKey: ["security-rules", selectedApp] });
        setIsAddRuleOpen(false);
      }
    }
  });

  const deleteRuleMutation = useMutation({
    mutationFn: (ruleId: string) => deleteSecurityRule(token!, selectedApp!, ruleId),
    onSuccess: (res) => {
      if (res.error) {
        toast.error(res.error);
      } else {
        toast.success("Security rule deleted");
        queryClient.invalidateQueries({ queryKey: ["security-rules", selectedApp] });
      }
    }
  });

  if (!mounted) {
    return (
      <DashboardLayout>
        <div className="flex flex-col gap-6 p-6">
          <Skeleton className="h-10 w-64" />
          <div className="grid gap-6 md:grid-cols-3">
            <Skeleton className="h-32 w-full" />
            <Skeleton className="h-32 w-full" />
            <Skeleton className="h-32 w-full" />
          </div>
          <Skeleton className="h-64 w-full" />
        </div>
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout>
      <div className="flex flex-col gap-6 p-6">
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-3">
            <div className="flex size-10 items-center justify-center rounded-md bg-primary text-primary-foreground">
              <Network />
            </div>
            <h1 className="text-3xl font-semibold tracking-tight">
              Networking (6PN)
            </h1>
          </div>
          <p className="text-sm text-muted-foreground">
            Manage your private L3 mesh network and Security Groups.
          </p>
        </div>

        <div className="grid gap-6 md:grid-cols-2 lg:grid-cols-3">
          <Card className="overflow-hidden border-2 transition-all hover:border-primary/20">
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2 bg-muted/30">
              <CardTitle className="text-sm font-bold uppercase tracking-wider">VPC Prefix</CardTitle>
              <HiOutlineGlobeAlt className="size-5 text-primary" />
            </CardHeader>
            <CardContent className="pt-4">
              {profileLoading ? (
                <Skeleton className="h-8 w-full" />
              ) : (
                <div className="text-2xl font-black font-mono break-all">
                  {profile?.vpc_ipv6_prefix || "Not assigned"}
                </div>
              )}
              <p className="text-xs text-muted-foreground mt-2 font-medium">
                Your private IPv6 /40 prefix for all applications.
              </p>
            </CardContent>
          </Card>

          <Card className="overflow-hidden border-2 transition-all hover:border-primary/20">
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2 bg-muted/30">
              <CardTitle className="text-sm font-bold uppercase tracking-wider">Active Peers</CardTitle>
              <HiServer className="size-5 text-primary" />
            </CardHeader>
            <CardContent className="pt-4">
              {meshLoading ? (
                <Skeleton className="h-8 w-16" />
              ) : (
                <div className="text-3xl font-black">{mesh?.total_workers || 0}</div>
              )}
              <p className="text-xs text-muted-foreground mt-2 font-medium">
                Agent nodes currently in your mesh network.
              </p>
            </CardContent>
          </Card>

          <Card className="overflow-hidden border-2 transition-all hover:border-primary/20">
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2 bg-muted/30">
              <CardTitle className="text-sm font-bold uppercase tracking-wider">Mesh Status</CardTitle>
              <Badge variant="outline" className="bg-emerald-500/10 text-emerald-500 border-emerald-500/20 font-bold">
                ENCRYPTED
              </Badge>
            </CardHeader>
            <CardContent className="pt-4">
              <div className="text-2xl font-black">WireGuard</div>
              <p className="text-xs text-muted-foreground mt-2 font-medium">
                All internal traffic is secured via mTLS and WireGuard.
              </p>
            </CardContent>
          </Card>
        </div>

        <Card className="border-2">
          <CardHeader className="border-b bg-muted/30">
            <div className="flex items-center justify-between">
              <div>
                <CardTitle className="text-lg font-bold flex items-center gap-2">
                  <HiOutlineCube className="size-5" />
                  Workload Connectivity
                </CardTitle>
                <CardDescription className="font-medium mt-1">
                  IPv6 addresses for your running microVMs.
                </CardDescription>
              </div>
            </div>
          </CardHeader>
          <CardContent className="p-0">
            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent bg-muted/20">
                  <TableHead className="font-bold uppercase text-[10px] tracking-widest px-6 h-10">Application</TableHead>
                  <TableHead className="font-bold uppercase text-[10px] tracking-widest h-10">VM ID</TableHead>
                  <TableHead className="font-bold uppercase text-[10px] tracking-widest h-10">IPv6 Address</TableHead>
                  <TableHead className="font-bold uppercase text-[10px] tracking-widest h-10">Host</TableHead>
                  <TableHead className="font-bold uppercase text-[10px] tracking-widest h-10 text-right pr-6">Status</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {deploymentsLoading ? (
                  Array.from({ length: 3 }).map((_, i) => (
                    <TableRow key={i}>
                      <TableCell colSpan={5} className="p-4">
                        <Skeleton className="h-12 w-full" />
                      </TableCell>
                    </TableRow>
                  ))
                ) : (deployments?.filter(d => d.status === "RUNNING")?.length || 0) === 0 ? (
                  <TableRow>
                    <TableCell colSpan={5} className="h-32 text-center text-muted-foreground font-medium">
                      No active workloads found.
                    </TableCell>
                  </TableRow>
                ) : (
                  deployments?.filter(d => d.status === "RUNNING").map((deployment) => (
                    <TableRow key={deployment.vm_id} className="group hover:bg-muted/50 transition-colors">
                      <TableCell className="px-6 font-bold">{deployment.app_name}</TableCell>
                      <TableCell className="font-mono text-xs">{deployment.vm_id.substring(0, 12)}</TableCell>
                      <TableCell>
                        <Badge variant="secondary" className="font-mono font-medium bg-primary/5 text-primary border-primary/10">
                          {deployment.ipv6_address || "Assigning..."}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground font-medium">{deployment.host_id}</TableCell>
                      <TableCell className="text-right pr-6">
                        <Badge className="bg-emerald-500/10 text-emerald-500 border-emerald-500/20 font-bold uppercase text-[10px]">
                          {deployment.status}
                        </Badge>
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </CardContent>
        </Card>

        <Card className="border-2">
          <CardHeader className="border-b bg-muted/30">
            <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
              <div>
                <CardTitle className="text-lg font-bold flex items-center gap-2">
                  <HiOutlineShieldCheck className="size-5" />
                  Security Groups
                </CardTitle>
                <CardDescription className="font-medium mt-1">
                  Distributed L3/L4 firewalling powered by eBPF.
                </CardDescription>
              </div>
              <div className="flex items-center gap-3">
                <Select onValueChange={(val) => setSelectedApp(val)}>
                  <SelectTrigger className="w-[200px] h-9 font-bold">
                    <SelectValue placeholder="Select Application" />
                  </SelectTrigger>
                  <SelectContent>
                    {apps?.map(app => (
                      <SelectItem key={app.id} value={app.name}>{app.name}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {selectedApp && (
                  <Dialog open={isAddRuleOpen} onOpenChange={setIsAddRuleOpen}>
                    <DialogTrigger asChild>
                      <Button size="sm" className="font-bold gap-2">
                        <HiPlus className="size-4" />
                        Add Rule
                      </Button>
                    </DialogTrigger>
                    <DialogContent>
                      <DialogHeader>
                        <DialogTitle>Add Security Rule</DialogTitle>
                        <DialogDescription>
                          Create a new firewall rule for <strong>{selectedApp}</strong>.
                        </DialogDescription>
                      </DialogHeader>
                      <div className="grid gap-4 py-4">
                        <div className="grid gap-2">
                          <Label htmlFor="protocol">Protocol</Label>
                          <Select 
                            value={newRule.protocol} 
                            onValueChange={(val) => setNewRule({...newRule, protocol: val})}
                          >
                            <SelectTrigger>
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="tcp">TCP</SelectItem>
                              <SelectItem value="udp">UDP</SelectItem>
                              <SelectItem value="any">Any</SelectItem>
                            </SelectContent>
                          </Select>
                        </div>
                        <div className="grid grid-cols-2 gap-4">
                          <div className="grid gap-2">
                            <Label htmlFor="port_start">Port Start</Label>
                            <Input 
                              id="port_start" 
                              type="number" 
                              value={newRule.port_start}
                              onChange={(e) => setNewRule({...newRule, port_start: parseInt(e.target.value)})}
                            />
                          </div>
                          <div className="grid gap-2">
                            <Label htmlFor="port_end">Port End</Label>
                            <Input 
                              id="port_end" 
                              type="number"
                              value={newRule.port_end}
                              onChange={(e) => setNewRule({...newRule, port_end: parseInt(e.target.value)})}
                            />
                          </div>
                        </div>
                        <div className="grid gap-2">
                          <Label htmlFor="action">Action</Label>
                          <Select 
                            value={newRule.action} 
                            onValueChange={(val) => setNewRule({...newRule, action: val})}
                          >
                            <SelectTrigger>
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="allow">ALLOW</SelectItem>
                              <SelectItem value="deny">DENY</SelectItem>
                            </SelectContent>
                          </Select>
                        </div>
                      </div>
                      <DialogFooter>
                        <Button 
                          onClick={() => createRuleMutation.mutate(newRule)}
                          disabled={createRuleMutation.isPending}
                          className="font-bold w-full"
                        >
                          {createRuleMutation.isPending ? "Creating..." : "Create Rule"}
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
              <div className="flex flex-col items-center justify-center h-48 text-muted-foreground p-6 text-center">
                <HiInformationCircle className="size-12 opacity-20 mb-4" />
                <p className="font-bold">Select an application to manage its Security Group rules.</p>
                <p className="text-sm mt-1">Rules are applied to all active microVMs of the selected app.</p>
              </div>
            ) : rulesLoading ? (
              <div className="p-6 space-y-4">
                <Skeleton className="h-10 w-full" />
                <Skeleton className="h-10 w-full" />
              </div>
            ) : rules?.length === 0 ? (
              <div className="flex flex-col items-center justify-center h-48 text-muted-foreground p-6 text-center">
                <p className="font-bold">No rules defined for this application.</p>
                <p className="text-sm mt-1">By default, all internal 6PN traffic is allowed.</p>
              </div>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow className="hover:bg-transparent bg-muted/20">
                    <TableHead className="font-bold uppercase text-[10px] tracking-widest px-6 h-10">Protocol</TableHead>
                    <TableHead className="font-bold uppercase text-[10px] tracking-widest h-10">Port Range</TableHead>
                    <TableHead className="font-bold uppercase text-[10px] tracking-widest h-10">Action</TableHead>
                    <TableHead className="font-bold uppercase text-[10px] tracking-widest h-10 text-right pr-6">Actions</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {rules?.map((rule) => (
                    <TableRow key={rule.id} className="group hover:bg-muted/50 transition-colors">
                      <TableCell className="px-6 font-black uppercase">{rule.protocol}</TableCell>
                      <TableCell className="font-mono">
                        {rule.protocol === "any" ? "All Ports" : rule.port_start === rule.port_end ? rule.port_start : `${rule.port_start}-${rule.port_end}`}
                      </TableCell>
                      <TableCell>
                        <Badge 
                          className={rule.action === "allow" 
                            ? "bg-emerald-500/10 text-emerald-500 border-emerald-500/20 font-black" 
                            : "bg-destructive/10 text-destructive border-destructive/20 font-black"
                          }
                        >
                          {rule.action.toUpperCase()}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-right pr-6">
                        <Button 
                          variant="ghost" 
                          size="icon" 
                          className="h-8 w-8 text-destructive hover:bg-destructive/10"
                          onClick={() => deleteRuleMutation.mutate(rule.id)}
                          disabled={deleteRuleMutation.isPending}
                        >
                          <HiTrash className="size-4" />
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
    </DashboardLayout>
  );
}
