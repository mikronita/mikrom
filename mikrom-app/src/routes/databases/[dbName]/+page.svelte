<script lang="ts">
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { 
    Database as DatabaseIcon, 
    ArrowLeft, 
    Radio, 
    Cpu, 
    HardDrive, 
    Shield, 
    Settings, 
    Terminal, 
    Globe2,
    ShieldCheck,
    Network,
    Server,
    Zap,
    ExternalLink,
    LockKeyhole,
    Eye,
    EyeOff,
    Copy,
    Check,
    Trash2,
    Activity,
    Clock
  } from "lucide-svelte";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import Card from "$lib/components/Card.svelte";
  import CardHeader from "$lib/components/CardHeader.svelte";
  import CardTitle from "$lib/components/CardTitle.svelte";
  import CardDescription from "$lib/components/CardDescription.svelte";
  import CardContent from "$lib/components/CardContent.svelte";
  import Badge from "$lib/components/Badge.svelte";
  import Button from "$lib/components/Button.svelte";
  import Select from "$lib/components/Select.svelte";
  import Field from "$lib/components/Field.svelte";
  import AlertDialog from "$lib/components/AlertDialog.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";
  import { databasesStore, deleteDatabase } from "$lib/stores/databases";
  import { toast } from "$lib/toast";
  import { formatDate } from "$lib/utils";

  const dbName = $page.params.dbName;
  $: db = $databasesStore.find(d => d.name === dbName);

  let activeTab: "overview" | "networking" | "backups" | "settings" = "overview";
  let showPassword = false;
  let copied = false;
  let showDeleteDialog = false;

  function copyToClipboard(text: string) {
    navigator.clipboard.writeText(text);
    copied = true;
    toast.success("Connection string copied to clipboard");
    setTimeout(() => (copied = false), 2000);
  }

  let maintenanceWindow = "sunday-02-00";

  function handleDelete() {
    if (db) {
      deleteDatabase(db.id);
      toast.success(`Database ${db.name} is being deleted`);
      goto("/databases");
    }
  }

  function getStatusBadgeClass(status: string) {
    switch (status) {
      case "Running":
        return "border-transparent bg-[color-mix(in_srgb,var(--status-online)_12%,transparent)] text-[var(--status-online)]";
      case "Provisioning":
        return "border-transparent bg-[color-mix(in_srgb,var(--status-info)_12%,transparent)] text-[var(--status-info)]";
      case "Deleting":
        return "border-transparent bg-[color-mix(in_srgb,var(--status-error)_12%,transparent)] text-[var(--status-error)]";
      default:
        return "border-transparent bg-muted/70 text-muted-foreground";
    }
  }

  const mockConnectedApps = [
    { name: "mikrom-api", ipv6: "fd00::1:1", status: "Running" },
    { name: "mikrom-scheduler", ipv6: "fd00::1:2", status: "Running" },
  ];
</script>

<svelte:head>
  <title>Mikrom - {dbName}</title>
</svelte:head>

<DashboardLayout>
  {#if !db}
    <div class="flex flex-col items-center justify-center py-20">
      <p class="text-muted-foreground">Database not found.</p>
      <Button variant="link" onclick={() => goto("/databases")}>Back to list</Button>
    </div>
  {:else}
    <div class="flex flex-col gap-6">
      <div class="flex flex-col gap-4">
        <div class="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
          <div class="flex items-center gap-4">
            <div class="flex size-12 items-center justify-center rounded-lg border border-border bg-background text-foreground">
              <DatabaseIcon class="size-6" />
            </div>
            <div class="flex flex-col">
              <div class="flex items-center gap-3">
                <h1 class="text-3xl font-semibold tracking-tight">{db.name}</h1>
                <Badge variant="outline" className={`gap-1.5 uppercase ${getStatusBadgeClass(db.status)}`}>
                  <Radio class="size-3" />
                  {db.status}
                </Badge>
              </div>
              <p class="text-sm text-muted-foreground">PostgreSQL {db.version} • {db.vcpus} vCPU • {db.memory_mib / 1024} GB RAM</p>
            </div>
          </div>
          <div class="flex items-center gap-2">
            <Badge variant="outline" className="gap-2 px-3 py-1.5 border-transparent bg-[color-mix(in_srgb,var(--status-info)_12%,transparent)] text-[var(--status-info)]">
              <LockKeyhole class="size-4" />
              Private 6PN
            </Badge>
          </div>
        </div>
      </div>

      <div class="grid gap-4 md:grid-cols-3">
        {#each [
          { label: "6PN address", value: `fd00::${db.id.split('-')[1]}`, description: "Internal IPv6 for peer connectivity.", icon: Globe2, valueClass: "break-all font-mono text-lg" },
          { label: "Storage used", value: `${Math.round(db.storage_gb * 0.15)} GB`, description: `Using 15% of your ${db.storage_gb} GB quota.`, icon: HardDrive, valueClass: "text-3xl" },
          { label: "Active links", value: mockConnectedApps.length, description: "MicroVMs with active 6PN routes to this DB.", icon: Network, valueClass: "text-3xl" },
        ] as card}
          <Card>
            <CardHeader class="flex flex-row items-start justify-between gap-4 pb-3">
              <div class="flex flex-col gap-1">
                <CardDescription>{card.label}</CardDescription>
                <CardTitle class={card.valueClass}>{card.value}</CardTitle>
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

      <div class="flex border-b border-border">
        <button
          class={`px-4 py-2 text-sm font-medium transition-colors border-b-2 ${activeTab === 'overview' ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground'}`}
          on:click={() => activeTab = 'overview'}
        >
          Overview
        </button>
        <button
          class={`px-4 py-2 text-sm font-medium transition-colors border-b-2 ${activeTab === 'networking' ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground'}`}
          on:click={() => activeTab = 'networking'}
        >
          Networking
        </button>
        <button
          class={`px-4 py-2 text-sm font-medium transition-colors border-b-2 ${activeTab === 'backups' ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground'}`}
          on:click={() => activeTab = 'backups'}
        >
          Backups
        </button>
        <button
          class={`px-4 py-2 text-sm font-medium transition-colors border-b-2 ${activeTab === 'settings' ? 'border-primary text-foreground' : 'border-transparent text-muted-foreground hover:text-foreground'}`}
          on:click={() => activeTab = 'settings'}
        >
          Settings
        </button>
      </div>

      {#if activeTab === 'overview'}
        <div class="grid gap-6 lg:grid-cols-[1fr_400px]">
          <div class="flex flex-col gap-6">
            <Card>
              <CardHeader>
                <div class="flex items-center gap-2">
                  <Terminal class="size-4 text-muted-foreground" />
                  <CardTitle class="text-base">Connection Details</CardTitle>
                </div>
                <CardDescription>Use these credentials to connect to your database over 6PN.</CardDescription>
              </CardHeader>
              <CardContent class="flex flex-col gap-4">
                <div class="grid gap-3">
                  <div class="flex flex-col gap-1.5">
                    <span class="text-xs font-medium text-muted-foreground">Connection String</span>
                    <div class="relative">
                      <div class="flex items-center rounded-md border border-border bg-muted/50 px-3 py-2 pr-20 font-mono text-xs overflow-hidden">
                        <span class="truncate">
                          {showPassword ? db.connection_string : db.connection_string.replace(/:[^@]+@/, ':••••••••@')}
                        </span>
                      </div>
                      <div class="absolute right-1 top-1/2 flex -translate-y-1/2 items-center gap-1">
                        <Button variant="ghost" size="icon" class="size-8" onclick={() => showPassword = !showPassword}>
                          {#if showPassword}
                            <EyeOff class="size-3.5" />
                          {:else}
                            <Eye class="size-3.5" />
                          {/if}
                        </Button>
                        <Button variant="ghost" size="icon" class="size-8" onclick={() => copyToClipboard(db.connection_string)}>
                          {#if copied}
                            <Check class="size-3.5 text-status-online" />
                          {:else}
                            <Copy class="size-3.5" />
                          {/if}
                        </Button>
                      </div>
                    </div>
                  </div>

                  <div class="grid grid-cols-2 gap-x-8 gap-y-4 pt-2">
                    <div class="flex flex-col gap-1">
                      <span class="text-xs font-medium text-muted-foreground">Host</span>
                      <span class="text-sm font-mono">{db.name}.mikrom.internal</span>
                    </div>
                    <div class="flex flex-col gap-1">
                      <span class="text-xs font-medium text-muted-foreground">Port</span>
                      <span class="text-sm font-mono">5432</span>
                    </div>
                    <div class="flex flex-col gap-1">
                      <span class="text-xs font-medium text-muted-foreground">Username</span>
                      <span class="text-sm font-mono">mikrom</span>
                    </div>
                    <div class="flex flex-col gap-1">
                      <span class="text-xs font-medium text-muted-foreground">Database</span>
                      <span class="text-sm font-mono">mikrom</span>
                    </div>
                  </div>
                </div>
              </CardContent>
            </Card>

            <Card>
              <CardHeader class="border-b border-border bg-muted/20">
                <div class="flex items-center justify-between gap-4">
                  <div class="flex flex-col gap-1.5">
                    <CardTitle class="text-base">Connectivity Links</CardTitle>
                    <CardDescription>Applications currently communicating with this database instance.</CardDescription>
                  </div>
                  <Badge variant="outline" className="border-transparent bg-[color-mix(in_srgb,var(--status-info)_12%,transparent)] text-[var(--status-info)]">
                    <Zap class="size-4" />
                    Live routes
                  </Badge>
                </div>
              </CardHeader>
              <div class="overflow-x-auto">
                <table class="w-full">
                  <thead>
                    <tr class="border-b border-border text-left text-xs uppercase tracking-wider text-muted-foreground">
                      <th class="px-4 py-3 font-medium">Source App</th>
                      <th class="px-4 py-3 font-medium">6PN Address</th>
                      <th class="px-4 py-3 text-right font-medium">Link Status</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each mockConnectedApps as app}
                      <tr class="border-b border-border last:border-0">
                        <td class="px-4 py-4">
                          <div class="flex items-center gap-3">
                            <div class="flex size-8 shrink-0 items-center justify-center rounded-md border bg-background text-muted-foreground">
                              <Server class="size-4" />
                            </div>
                            <span class="font-medium">{app.name}</span>
                          </div>
                        </td>
                        <td class="px-4 py-4">
                          <span class="rounded-md border bg-muted/40 px-2 py-1 font-mono text-xs">{app.ipv6}</span>
                        </td>
                        <td class="px-4 py-4 text-right">
                          <Badge variant="outline" className="border-transparent bg-[color-mix(in_srgb,var(--status-online)_12%,transparent)] text-[var(--status-online)]">
                            {app.status}
                          </Badge>
                        </td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            </Card>
          </div>

          <div class="flex flex-col gap-6">
            <Card>
              <CardHeader>
                <div class="flex items-center gap-2">
                  <Activity class="size-4 text-muted-foreground" />
                  <CardTitle class="text-base">Resource Usage</CardTitle>
                </div>
                <CardDescription>Current compute and storage allocation.</CardDescription>
              </CardHeader>
              <CardContent class="flex flex-col gap-6">
                <div class="grid grid-cols-2 gap-4">
                  <div class="flex flex-col gap-1">
                    <div class="flex items-center gap-2 text-xs font-medium text-muted-foreground">
                      <Cpu class="size-3" />
                      vCPU
                    </div>
                    <span class="text-lg font-semibold">{db.vcpus} Cores</span>
                  </div>
                  <div class="flex flex-col gap-1">
                    <div class="flex items-center gap-2 text-xs font-medium text-muted-foreground">
                      <HardDrive class="size-3" />
                      Storage
                    </div>
                    <span class="text-lg font-semibold">{db.storage_gb} GB</span>
                  </div>
                </div>
                <div class="flex flex-col gap-1">
                  <div class="flex items-center gap-2 text-xs font-medium text-muted-foreground">
                    <DatabaseIcon class="size-3" />
                    Memory
                  </div>
                  <span class="text-lg font-semibold">{db.memory_mib / 1024} GB RAM</span>
                </div>
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle class="text-base">Quick Help</CardTitle>
              </CardHeader>
              <CardContent class="text-sm text-muted-foreground">
                <p>Internal databases are only reachable via the private 6PN mesh. Ensure your application has the correct security rules allowed in Networking.</p>
                <Button variant="link" class="h-auto p-0 pt-4 text-primary" onclick={() => goto("/networking")}>
                  Manage Mesh Networking
                  <ExternalLink class="ml-1 size-3" />
                </Button>
              </CardContent>
            </Card>
          </div>
        </div>
      {:else if activeTab === 'networking'}
        <Card>
          <CardHeader>
            <div class="flex items-center gap-2">
              <ShieldCheck class="size-4 text-muted-foreground" />
              <CardTitle class="text-base">Access Control</CardTitle>
            </div>
            <CardDescription>Manage which resources can connect to this database.</CardDescription>
          </CardHeader>
          <CardContent>
             <EmptyState class="py-12">
              <LockKeyhole class="size-10 text-muted-foreground" />
              <h3 class="text-xl font-semibold">Mesh Security</h3>
              <p class="max-w-md text-sm text-muted-foreground">By default, all applications in your VPC can connect to this database via port 5432.</p>
              <Button variant="outline" size="sm" class="mt-4" disabled>
                Configure Custom Rules (Soon)
              </Button>
            </EmptyState>
          </CardContent>
        </Card>
      {:else if activeTab === 'backups'}
        <Card>
          <CardHeader>
            <div class="flex items-center gap-2">
              <Clock class="size-4 text-muted-foreground" />
              <CardTitle class="text-base">Automated Backups</CardTitle>
            </div>
            <CardDescription>Daily snapshots of your database.</CardDescription>
          </CardHeader>
          <CardContent>
            <div class="flex flex-col gap-1">
              {#each Array.from({ length: 3 }) as _, i}
                <div class="flex items-center justify-between border-b border-border py-3 last:border-0">
                  <div class="flex flex-col gap-0.5">
                    <span class="text-sm font-medium">Backup-{new Date(Date.now() - i * 86400000).toISOString().split('T')[0]}</span>
                    <span class="text-xs text-muted-foreground">{formatDate(new Date(Date.now() - i * 86400000).toISOString())}</span>
                  </div>
                  <Button variant="outline" size="sm">Restore</Button>
                </div>
              {/each}
            </div>
          </CardContent>
        </Card>
      {:else if activeTab === 'settings'}
        <div class="flex flex-col gap-6">
          <Card>
            <CardHeader>
              <div class="flex items-center gap-2">
                <Settings class="size-4 text-muted-foreground" />
                <CardTitle class="text-base">Database Configuration</CardTitle>
              </div>
              <CardDescription>Manage your database settings and performance.</CardDescription>
            </CardHeader>
            <CardContent class="flex flex-col gap-4">
              <Field label="Maintenance Window" description="When automated updates and backups are performed.">
                <Select bind:value={maintenanceWindow}>
                  <option value="sunday-02-00">Sunday at 02:00 UTC</option>
                  <option value="saturday-02-00">Saturday at 02:00 UTC</option>
                </Select>
              </Field>
            </CardContent>
          </Card>

          <Card class="border-destructive/20 bg-destructive/5">
            <CardHeader>
              <div class="flex items-center gap-2 text-destructive">
                <Trash2 class="size-4" />
                <CardTitle class="text-base">Danger Zone</CardTitle>
              </div>
              <CardDescription>Irreversible actions for this database.</CardDescription>
            </CardHeader>
            <CardContent>
              <div class="flex items-center justify-between">
                <div class="flex flex-col gap-1">
                  <span class="text-sm font-medium">Delete Database</span>
                  <p class="text-xs text-muted-foreground">This will permanently delete the database and all its backups.</p>
                </div>
                <Button variant="destructive" size="sm" onclick={() => (showDeleteDialog = true)}>Delete Database</Button>
              </div>
            </CardContent>
          </Card>
        </div>
      {/if}
    </div>
  {/if}

  <AlertDialog
    bind:open={showDeleteDialog}
    title="Are you absolutely sure?"
    description="This action cannot be undone. This will permanently delete your database and all associated data."
    confirmLabel="Delete Database"
    confirmVariant="destructive"
    on:confirm={handleDelete}
  />
</DashboardLayout>
