<script lang="ts">
  import { onMount } from "svelte";
  import { Boxes, Globe2, LockKeyhole, Network, Plus, Server, ShieldCheck, Trash2 } from "lucide-svelte";
  import {
    Card,
    CardHeader,
    CardTitle,
    CardDescription,
    CardContent,
    Badge,
    Button,
    AlertDialog,
    EmptyState,
    Skeleton,
    Modal,
    Field,
    Input,
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
    Table,
    TableBody,
    TableCell,
    TableHead,
    TableHeader,
    TableRow,
  } from "$lib/components";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import { getToken } from "$lib/auth";
  import {
    createSecurityRule,
    deleteSecurityRule,
    getMeshStatus,
    type CreateSecurityRuleRequest,
    type MeshStatus,
    type SecurityRule,
    watchMeshStatus,
  } from "$lib/api";
  import { vmsStore } from "$lib/stores/vms";
  import { toast } from "$lib/toast";
  import { appsStore, refreshApps } from "$lib/stores/apps";
  import { profile, refreshProfile } from "$lib/stores/profile";
  import { securityRulesStore, securityRulesLoading, refreshSecurityRules } from "$lib/stores/networking";

  const defaultRule: CreateSecurityRuleRequest = { protocol: "tcp", port_start: 80, port_end: 80, action: "allow" };

  let mesh: MeshStatus | null = null;
  let selectedApp = "";
  let loading = true;
  let showRuleModal = false;
  let rule: CreateSecurityRuleRequest = defaultRule;
  let ruleToDelete: SecurityRule | null = null;

  function formatVmId(vmId: string) {
    return vmId.length > 12 ? vmId.substring(0, 12) : vmId;
  }

  function formatPortRange(item: { protocol: string; port_start: number; port_end: number }) {
    if (item.protocol === "any") return "All ports";
    return item.port_start === item.port_end ? `${item.port_start}` : `${item.port_start}-${item.port_end}`;
  }

  function getDeploymentBadgeProps(status: string) {
    const s = status.toLowerCase();
    if (s === "running") {
      return {
        variant: "outline" as const,
        className: "border-transparent bg-status-info/10 text-status-info",
      };
    }
    if (s === "draining" || s === "building" || s === "scheduled" || s === "pending" || s === "paused") {
      return {
        variant: "outline" as const,
        className: "border-transparent bg-status-warning/10 text-status-warning",
      };
    }
    if (s === "failed" || s === "cancelled") {
      return { variant: "destructive" as const, className: "" };
    }
    return { variant: "outline" as const, className: "" };
  }

  async function loadRules(token: string, appName: string) {
    await refreshSecurityRules(appName);
  }

  onMount(() => {
    const token = getToken();
    if (!token) return;

    void (async () => {
      try {
        const results = await Promise.all([
          refreshProfile(),
          getMeshStatus(token),
          refreshApps(),
        ]);

        const meshResult = results[1];
        if (meshResult.data) mesh = meshResult.data;
        if (meshResult.error) toast.error(meshResult.error);

        if ($appsStore.length > 0) {
          selectedApp = $appsStore[0].name;
          await loadRules(token, selectedApp);
        }
      } catch (_err) {
        toast.error("Failed to load networking data");
      } finally {
        loading = false;
      }
    })();

    const cleanupMesh = watchMeshStatus(token, (data) => {
      mesh = data;
    });

    return () => {
      cleanupMesh();
    };
  });

  $: runningDeployments = $vmsStore.filter((deployment) => deployment.status === "RUNNING");

  async function createRule() {
    const token = getToken();
    if (!token || !selectedApp) return;
    const result = await createSecurityRule(token, selectedApp, rule);
    if (result.error) {
      toast.error(result.error);
      return;
    }
    toast.success("Security rule created");
    rule = defaultRule;
    showRuleModal = false;
    await loadRules(token, selectedApp);
  }

  async function removeRule(id: string) {
    const token = getToken();
    if (!token || !selectedApp) return;
    const result = await deleteSecurityRule(token, selectedApp, id);
    if (result.error) {
      toast.error(result.error);
      return;
    }
    toast.success("Security rule deleted");
    await loadRules(token, selectedApp);
  }

  async function confirmDeleteRule() {
    if (!ruleToDelete) return;
    const target = ruleToDelete;
    ruleToDelete = null;
    await removeRule(target.id);
  }
</script>

<svelte:head>
  <title>Mikrom - Networking</title>
</svelte:head>

<DashboardLayout>
  <div class="flex flex-col gap-6">
    <div class="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
      <div class="flex flex-col gap-2">
        <div class="flex items-center gap-3">
          <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
            <Network />
          </div>
          <h1 class="text-3xl font-semibold tracking-tight">Networking</h1>
        </div>
        <p class="max-w-2xl text-sm text-muted-foreground">Monitor the private 6PN mesh, workload addresses and application security rules.</p>
      </div>
      <Badge variant="outline" class="w-fit gap-2 px-3 py-1.5 border-transparent bg-status-info/10 text-status-info">
        <LockKeyhole class="size-4" />
        WireGuard mesh
      </Badge>
    </div>

    <div class="grid gap-4 md:grid-cols-3">
      {#each [
        { label: "VPC prefix", value: $profile?.vpc_ipv6_prefix || "Not assigned", description: "Private IPv6 /40 prefix reserved for your applications.", icon: Globe2, loading: !$profile, valueClass: "break-all font-mono text-lg" },
        { label: "Active peers", value: (mesh?.total_workers ?? 0) + runningDeployments.length, description: "Total nodes and microVMs participating in the mesh.", icon: Server, loading: !mesh || loading, valueClass: "text-3xl" },
        { label: "Running workloads", value: runningDeployments.length, description: "MicroVMs currently reachable through 6PN.", icon: Boxes, loading: loading, valueClass: "text-3xl" },
      ] as card}
        <Card>
          <CardHeader class="flex flex-row items-start justify-between gap-4 pb-3">
            <div class="flex flex-col gap-1">
              <CardDescription>{card.label}</CardDescription>
              {#if card.loading}
                <Skeleton class={`mt-1 h-8 ${card.valueClass.includes("break-all") ? "w-32" : "w-24"}`} />
              {:else}
                <CardTitle class={card.valueClass}>{card.value}</CardTitle>
              {/if}
            </div>
            <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
              <svelte:component this={card.icon} class="size-5" />
            </div>
          </CardHeader>
          <CardContent class="pt-0">
            <p class="text-sm text-muted-foreground">{card.description}</p>
          </CardContent>
        </Card>
      {/each}
    </div>

    <div class="grid gap-4 xl:grid-cols-[minmax(0,1.15fr)_minmax(24rem,0.85fr)]">
      <Card class="h-fit">
        <CardHeader class="border-b border-border bg-muted/20">
          <div class="flex items-center justify-between gap-4">
            <div class="grid gap-1">
              <CardTitle>Workload connectivity</CardTitle>
              <CardDescription>Running microVMs reachable inside your private 6PN mesh.</CardDescription>
            </div>
            <Badge variant="outline" class="border-transparent bg-status-info/10 text-status-info">
              <Network class="size-4" />
              {runningDeployments.length} active
            </Badge>
          </div>
        </CardHeader>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Workload</TableHead>
              <TableHead>6PN address</TableHead>
              <TableHead class="text-right">Health</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {#if loading}
              {#each Array.from({ length: 3 }) as _}
                <TableRow>
                  <TableCell colspan={3}><Skeleton class="h-10 w-full" /></TableCell>
                </TableRow>
              {/each}
            {:else if runningDeployments.length === 0}
              <TableRow>
                <TableCell colspan={3}>
                  <EmptyState>
                    <Network class="size-10 text-muted-foreground" />
                    <h3 class="text-xl font-semibold">No active workloads</h3>
                    <p class="text-sm text-muted-foreground">Running deployments will appear here with their private network address.</p>
                  </EmptyState>
                </TableCell>
              </TableRow>
            {:else}
              {#each runningDeployments as deployment}
                {@const deploymentBadge = getDeploymentBadgeProps(deployment.status)}
                <TableRow>
                  <TableCell>
                    <a href={`/apps/${encodeURIComponent(deployment.app_name)}`} class="flex items-center gap-3 hover:opacity-80">
                      <div class="flex size-9 shrink-0 items-center justify-center rounded-md border bg-background text-muted-foreground">
                        <Boxes class="size-4" />
                      </div>
                      <div>
                        <div class="truncate font-medium">{deployment.app_name}</div>
                        <div class="font-mono text-xs text-muted-foreground">vm-{formatVmId(deployment.vm_id)}</div>
                      </div>
                    </a>
                  </TableCell>
                  <TableCell>
                    <div class="flex flex-col gap-1">
                      <span class="w-fit rounded-md border bg-muted/40 px-2 py-1 font-mono text-xs">{deployment.ipv6_address || "Assigning address..."}</span>
                      <span class="text-xs text-muted-foreground">Private mesh endpoint</span>
                    </div>
                  </TableCell>
                  <TableCell class="text-right">
                    <Badge variant={deploymentBadge.variant} class={`capitalize ${deploymentBadge.className}`}>
                      {deployment.status.toLowerCase()}
                    </Badge>
                  </TableCell>
                </TableRow>
              {/each}
            {/if}
          </TableBody>
        </Table>
      </Card>

      <Card>
        <CardHeader class="border-b border-border bg-muted/20">
          <div class="flex flex-col gap-4">
            <div class="grid gap-1">
              <CardTitle>Security groups</CardTitle>
              <CardDescription>L3/L4 rules applied to every active microVM for an application.</CardDescription>
            </div>
            <div class="flex flex-col gap-2 sm:flex-row">
              <Select bind:value={selectedApp} onValueChange={async (val: string | undefined) => {
                if (val) {
                  selectedApp = val;
                  const token = getToken();
                  if (token && selectedApp) await loadRules(token, selectedApp);
                }
              }}>
                <SelectTrigger>
                  <SelectValue placeholder="Select application" />
                </SelectTrigger>
                <SelectContent>
                  {#each $appsStore as app}
                    <SelectItem value={app.name}>{app.name}</SelectItem>
                  {/each}
                </SelectContent>
              </Select>
              {#if selectedApp}
                <Button size="sm" onclick={() => (showRuleModal = true)}>
                  <Plus class="size-4" />
                  Add rule
                </Button>
              {/if}
            </div>
          </div>
        </CardHeader>

        <CardContent class="pt-5">
          {#if !selectedApp}
            <EmptyState>
              <ShieldCheck class="size-10 text-muted-foreground" />
              <h3 class="text-xl font-semibold">Select an application</h3>
              <p class="text-sm text-muted-foreground">Choose an app to inspect and manage its security group rules.</p>
            </EmptyState>
          {:else if $securityRulesLoading}
            <div class="flex flex-col gap-3">
              <Skeleton class="h-10 w-full" />
              <Skeleton class="h-10 w-full" />
              <Skeleton class="h-10 w-full" />
            </div>
          {:else if $securityRulesStore.length === 0}
            <EmptyState>
              <ShieldCheck class="size-10 text-muted-foreground" />
              <h3 class="text-xl font-semibold">No security rules</h3>
              <p class="text-sm text-muted-foreground">Create the first firewall rule for this application.</p>
            </EmptyState>
          {:else}
            <div class="space-y-2">
              {#each $securityRulesStore as item}
                <div class="flex items-center justify-between gap-4 rounded-md border border-border bg-muted/20 p-3">
                  <div>
                    <div class="font-medium">{item.protocol.toUpperCase()} {formatPortRange(item)}</div>
                    <div class="text-xs text-muted-foreground">Priority {item.priority}</div>
                  </div>
                  <div class="flex items-center gap-2">
                    <Badge variant={item.action === "allow" ? "outline" : "destructive"} class={item.action === "allow" ? "border-transparent bg-status-info/10 text-status-info" : ""}>{item.action}</Badge>
                    <Button variant="ghost" size="icon" onclick={() => (ruleToDelete = item)}>
                      <Trash2 class="size-4" />
                    </Button>
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </CardContent>
      </Card>
    </div>
  </div>

  {#if showRuleModal}
    <Modal bind:open={showRuleModal} title="Add security rule" description={`Create a firewall rule for ${selectedApp}.`}>
      <div class="space-y-4">
        <Field label="Protocol">
          <Select bind:value={rule.protocol}>
            <SelectTrigger>
              <SelectValue placeholder="Select protocol" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="tcp">TCP</SelectItem>
              <SelectItem value="udp">UDP</SelectItem>
              <SelectItem value="any">Any</SelectItem>
            </SelectContent>
          </Select>
        </Field>
        <div class="grid gap-4 sm:grid-cols-2">
          <Field label="Port start"><Input type="number" bind:value={rule.port_start} min={0} max={65535} disabled={rule.protocol === "any"} /></Field>
          <Field label="Port end"><Input type="number" bind:value={rule.port_end} min={0} max={65535} disabled={rule.protocol === "any"} /></Field>
        </div>
        <Field label="Action">
          <Select bind:value={rule.action}>
            <SelectTrigger>
              <SelectValue placeholder="Select action" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="allow">Allow</SelectItem>
              <SelectItem value="deny">Deny</SelectItem>
            </SelectContent>
          </Select>
        </Field>
        <div class="flex justify-end gap-2">
          <Button variant="outline" onclick={() => (showRuleModal = false)}>Cancel</Button>
          <Button onclick={createRule}>Create rule</Button>
        </div>
      </div>
    </Modal>
  {/if}

  <AlertDialog
    open={Boolean(ruleToDelete)}
    title="Delete security rule?"
    description={`This will remove the ${ruleToDelete?.protocol.toUpperCase() || "selected"} rule for ${selectedApp} and cannot be undone.`}
    actionText="Delete rule"
    onclose={() => (ruleToDelete = null)}
    onaction={confirmDeleteRule}
  />
</DashboardLayout>
