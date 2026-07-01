<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/stores";
  import {
    Database as DatabaseIcon,
    Radio,
    Cpu,
    HardDrive,
    Camera,
    Terminal,
    Globe2,
    Copy,
    Check,
    RotateCcw,
    Trash2,
    Server,
    ArrowLeft,
  } from "lucide-svelte";
  import {
    Card,
    CardHeader,
    CardTitle,
    CardDescription,
    CardContent,
    Badge,
    Button,
    EmptyState,
    AlertDialog,
    SectionTabs,
    Input,
  } from "$lib/components";
  import type {
    DatabaseBackupInfo,
    DatabaseBranchInfo,
    DatabaseConnectionInfo,
    DatabaseSnapshotInfo,
  } from "$lib/api";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import { getToken } from "$lib/auth";
  import {
    databasesStore,
    deleteDatabase,
    getDatabaseConnection,
    refreshDatabases,
    type Database,
  } from "$lib/stores/databases";
  import { get } from "svelte/store";
  import { toast } from "$lib/toast";
  import { formatDate } from "$lib/utils";
  import {
    createDatabaseSnapshot,
    deleteDatabaseSnapshot,
    getDatabaseBackupInfo,
    listDatabaseBranches,
    listDatabaseSnapshots,
    restoreDatabaseSnapshot,
  } from "$lib/api";

  let dbName = "";
  $: dbName = decodeURIComponent($page.params.dbName ?? "");
  let db: Database | null = null;

  let connectionInfo: DatabaseConnectionInfo | null = null;
  let connectionLoading = false;
  let connectionError = "";
  let branches: DatabaseBranchInfo[] = [];
  let branchesLoading = false;
  let branchesError = "";
  let backupInfo: DatabaseBackupInfo | null = null;
  let backupLoading = false;
  let backupError = "";
  let snapshots: DatabaseSnapshotInfo[] = [];
  let snapshotsLoading = false;
  let snapshotsError = "";
  let snapshotName = "";
  let snapshotActionLoading = false;
  let snapshotActionError = "";
  let restoreSnapshotTarget = "";
  let deleteSnapshotTarget = "";
  let showDeleteDialog = false;
  let showRestoreSnapshotDialog = false;
  let showDeleteSnapshotDialog = false;
  let copiedKey: "ssh" | "psql" | null = null;
  let lastLoadedDatabaseId: string | null = null;
  let detailsLoading = false;
  let activeTab: "overview" | "connection" | "branches" | "backups" = "overview";

  const databaseTabs = [
    { value: "overview", label: "Overview" },
    { value: "connection", label: "Connection" },
    { value: "branches", label: "Branches" },
    { value: "backups", label: "Backups" },
  ] as const;

  function getStatusBadgeClass(status: string) {
    switch (status) {
      case "Running":
        return "border-transparent bg-status-online/10 text-status-online";
      case "Provisioning":
        return "border-transparent bg-status-info/10 text-status-info";
      case "Deleting":
      case "Failed":
        return "border-transparent bg-status-offline/10 text-status-offline";
      default:
        return "border-transparent bg-muted/70 text-muted-foreground";
    }
  }

  function copyToClipboard(text: string, kind: "ssh" | "psql") {
    navigator.clipboard.writeText(text);
    copiedKey = kind;
    toast.success(`${kind === "ssh" ? "SSH tunnel" : "psql command"} copied to clipboard`);
    setTimeout(() => {
      if (copiedKey === kind) copiedKey = null;
    }, 2000);
  }

  function formatBytes(sizeBytes: number) {
    if (sizeBytes >= 1024 * 1024 * 1024) {
      return `${(sizeBytes / (1024 * 1024 * 1024)).toFixed(1)} GiB`;
    }

    if (sizeBytes >= 1024 * 1024) {
      return `${(sizeBytes / (1024 * 1024)).toFixed(1)} MiB`;
    }

    if (sizeBytes >= 1024) {
      return `${(sizeBytes / 1024).toFixed(1)} KiB`;
    }

    return `${sizeBytes} B`;
  }

  function formatBackupError(message: string) {
    if (message.includes("Database has no active deployment yet")) {
      return "This database needs an active deployment before you can use snapshots. Provision or deploy it first.";
    }

    return message;
  }

  async function loadConnectionInfo(databaseId: string) {
    connectionLoading = true;
    connectionError = "";
    const result = await getDatabaseConnection(databaseId);
    connectionLoading = false;
    if (result.error) {
      connectionError = result.error;
      connectionInfo = null;
      detailsLoading = false;
      return;
    }

    connectionInfo = result.data ?? null;
    detailsLoading = false;
  }

  async function loadBranches(databaseId: string) {
    branchesLoading = true;
    branchesError = "";
    const token = getToken();
    if (!token) {
      branchesLoading = false;
      branchesError = "You must be logged in";
      branches = [];
      return;
    }

    const result = await listDatabaseBranches(token, databaseId);
    branchesLoading = false;
    if (result.error) {
      branchesError = result.error;
      branches = [];
      return;
    }

    branches = result.data ?? [];
  }

  async function loadBackups(databaseId: string) {
    backupLoading = true;
    backupError = "";
    const token = getToken();
    if (!token) {
      backupLoading = false;
      backupError = "You must be logged in";
      backupInfo = null;
      return;
    }

    const result = await getDatabaseBackupInfo(token, databaseId);
    backupLoading = false;
    if (result.error) {
      backupError = formatBackupError(result.error);
      backupInfo = null;
      return;
    }

    backupInfo = result.data ?? null;
  }

  async function loadSnapshots(databaseId: string) {
    snapshotsLoading = true;
    snapshotsError = "";
    const token = getToken();
    if (!token) {
      snapshotsLoading = false;
      snapshotsError = "You must be logged in";
      snapshots = [];
      return;
    }

    const result = await listDatabaseSnapshots(token, databaseId);
    snapshotsLoading = false;
    if (result.error) {
      snapshotsError = formatBackupError(result.error);
      snapshots = [];
      return;
    }

    const data = result.data ?? { success: false, message: "No data returned", snapshots: [] };
    snapshots = data.snapshots ?? [];
    if (!data.success && data.message) {
      snapshotsError = data.message;
    }
  }

  function syncDatabaseState(entries: Database[]) {
    const nextDb = entries.find((entry) => entry.name === dbName) ?? null;
    db = nextDb;
    if (!nextDb) {
      detailsLoading = false;
      return;
    }

    if (nextDb.id !== lastLoadedDatabaseId) {
      lastLoadedDatabaseId = nextDb.id;
      connectionInfo = null;
      connectionError = "";
      branches = [];
      branchesError = "";
      backupInfo = null;
      backupError = "";
      snapshots = [];
      snapshotsError = "";
      detailsLoading = true;
      void loadConnectionInfo(nextDb.id);
    }
  }

  async function createSnapshot() {
    if (!db || !snapshotName.trim()) return;

    const token = getToken();
    if (!token) {
      snapshotActionError = "You must be logged in";
      return;
    }

    snapshotActionLoading = true;
    snapshotActionError = "";
    const result = await createDatabaseSnapshot(token, db.id, { name: snapshotName.trim() });
    snapshotActionLoading = false;
    if (result.error) {
      snapshotActionError = formatBackupError(result.error);
      toast.error(snapshotActionError);
      return;
    }

    const action = result.data;
    if (action && !action.success) {
      snapshotActionError = formatBackupError(action.message);
      toast.error(snapshotActionError);
      return;
    }

    snapshotName = "";
    toast.success(action?.message || "Snapshot created");
    void loadSnapshots(db.id);
  }

  async function restoreSnapshot() {
    if (!db || !restoreSnapshotTarget) return;

    const token = getToken();
    if (!token) {
      snapshotActionError = "You must be logged in";
      return;
    }

    snapshotActionLoading = true;
    snapshotActionError = "";
    const result = await restoreDatabaseSnapshot(token, db.id, { snapshot_name: restoreSnapshotTarget });
    snapshotActionLoading = false;
    if (result.error) {
      snapshotActionError = formatBackupError(result.error);
      toast.error(snapshotActionError);
      return;
    }

    const action = result.data;
    if (action && !action.success) {
      snapshotActionError = formatBackupError(action.message);
      toast.error(snapshotActionError);
      return;
    }

    toast.success(action?.message || "Snapshot restored");
    showRestoreSnapshotDialog = false;
    restoreSnapshotTarget = "";
  }

  async function deleteSnapshot() {
    if (!db || !deleteSnapshotTarget) return;

    const token = getToken();
    if (!token) {
      snapshotActionError = "You must be logged in";
      return;
    }

    snapshotActionLoading = true;
    snapshotActionError = "";
    const result = await deleteDatabaseSnapshot(token, db.id, deleteSnapshotTarget);
    snapshotActionLoading = false;
    if (result.error) {
      snapshotActionError = formatBackupError(result.error);
      toast.error(snapshotActionError);
      return;
    }

    const action = result.data;
    if (action && !action.success) {
      snapshotActionError = formatBackupError(action.message);
      toast.error(snapshotActionError);
      return;
    }

    toast.success(action?.message || "Snapshot deleted");
    showDeleteSnapshotDialog = false;
    deleteSnapshotTarget = "";
    void loadSnapshots(db.id);
  }

  async function handleDelete() {
    if (!db) return;

    const result = await deleteDatabase(db.id);
    if (!result.success) {
      toast.error(result.error || "Failed to delete database");
      return;
    }

    toast.success(`Database ${db.name} is being deleted`);
    goto("/databases");
  }

  onMount(() => {
    if ($databasesStore.length === 0) {
      void refreshDatabases();
    }
    const unsubscribeDatabases = databasesStore.subscribe((entries) => {
      syncDatabaseState(entries);
    });

    const unsubscribePage = page.subscribe(($page) => {
      dbName = decodeURIComponent($page.params.dbName ?? "");
      syncDatabaseState(get(databasesStore));
    });

    syncDatabaseState(get(databasesStore));

    return () => {
      unsubscribeDatabases();
      unsubscribePage();
    };
  });

  function handleTabChange(value: string) {
    if (!db) return;

    if (value === "branches" && branches.length === 0 && !branchesLoading && !branchesError) {
      void loadBranches(db.id);
    }

    if (value === "backups") {
      if (backupInfo === null && !backupLoading && !backupError) {
        void loadBackups(db.id);
      }

      if (snapshots.length === 0 && !snapshotsLoading && !snapshotsError) {
        void loadSnapshots(db.id);
      }
    }
  }
</script>

<svelte:head>
  <title>Mikrom - {dbName}</title>
</svelte:head>

<DashboardLayout>
  {#if !db}
    <div class="flex flex-col items-center justify-center gap-4 py-20">
      <EmptyState class="py-0">
        <DatabaseIcon class="size-10 text-muted-foreground" />
        <h2 class="text-xl font-semibold">Database not found</h2>
        <p class="max-w-md text-sm text-muted-foreground">
          We could not find a database named {dbName} in the active project.
        </p>
        <Button variant="outline" onclick={() => goto("/databases")}>
          <ArrowLeft class="size-4" />
          Back to databases
        </Button>
      </EmptyState>
    </div>
  {:else}
    <div class="flex flex-col gap-6">
      <div class="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
        <div class="flex items-center gap-4">
          <div class="flex size-12 items-center justify-center rounded-md border border-border bg-background text-foreground">
            <DatabaseIcon class="size-6" />
          </div>
          <div class="flex flex-col gap-1">
            <div class="flex flex-wrap items-center gap-3">
              <h1 class="text-3xl font-semibold tracking-tight">{db.name}</h1>
              <Badge variant="outline" class={`gap-1.5 uppercase ${getStatusBadgeClass(db.status)}`}>
                <Radio class="size-3" />
                {db.status}
              </Badge>
            </div>
            <p class="text-sm text-muted-foreground">
              PostgreSQL {db.postgres_version} · {db.vcpus} vCPU · {Math.max(1, Math.round(db.disk_mib / 1024))} GB storage
            </p>
          </div>
        </div>
        <Button variant="destructive" onclick={() => (showDeleteDialog = true)}>
          <Trash2 class="size-4" />
          Delete Database
        </Button>
      </div>

      <div class="grid gap-4 md:grid-cols-3">
        <Card size="sm">
          <CardHeader class="flex flex-row items-start justify-between gap-4">
            <div class="flex flex-col gap-1">
              <CardDescription>PostgreSQL version</CardDescription>
              <CardTitle class="text-2xl">PostgreSQL {db.postgres_version}</CardTitle>
            </div>
            <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
              <Globe2 class="size-5" />
            </div>
          </CardHeader>
          <CardContent>
            <p class="text-sm text-muted-foreground">Version selected when the database was provisioned.</p>
          </CardContent>
        </Card>

        <Card size="sm">
          <CardHeader class="flex flex-row items-start justify-between gap-4">
            <div class="flex flex-col gap-1">
              <CardDescription>Storage quota</CardDescription>
              <CardTitle class="text-2xl">{Math.max(1, Math.round(db.disk_mib / 1024))} GB</CardTitle>
            </div>
            <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
              <HardDrive class="size-5" />
            </div>
          </CardHeader>
          <CardContent>
            <p class="text-sm text-muted-foreground">Allocated volume size for this Neon database.</p>
          </CardContent>
        </Card>

        <Card size="sm">
          <CardHeader class="flex flex-row items-start justify-between gap-4">
            <div class="flex flex-col gap-1">
              <CardDescription>Compute</CardDescription>
              <CardTitle class="text-2xl">{db.vcpus} vCPU</CardTitle>
            </div>
            <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
              <Cpu class="size-5" />
            </div>
          </CardHeader>
          <CardContent>
            <p class="text-sm text-muted-foreground">Provisioned compute for the backing microVM.</p>
          </CardContent>
        </Card>
      </div>

      <SectionTabs bind:active={activeTab} tabs={databaseTabs} onChange={handleTabChange} />

      {#if detailsLoading}
        <Card>
          <CardContent class="flex flex-col gap-3 py-10">
            <p class="text-sm font-medium">Loading database details</p>
            <p class="text-sm text-muted-foreground">
              Fetching connection, branch, and backup information for this database.
            </p>
          </CardContent>
        </Card>
      {:else if activeTab === "overview"}
        <div class="grid gap-6 lg:grid-cols-[1fr_360px]">
          <Card>
            <CardHeader>
              <div class="flex items-center gap-2">
                <Server class="size-4 text-muted-foreground" />
                <CardTitle class="text-base">Database Facts</CardTitle>
              </div>
              <CardDescription>Key metadata returned by the control plane.</CardDescription>
            </CardHeader>
            <CardContent class="flex flex-col gap-4 text-sm">
              <div class="flex items-center justify-between gap-4">
                <span class="text-muted-foreground">Created</span>
                <span>{formatDate(db.created_at)}</span>
              </div>
              <div class="flex items-center justify-between gap-4">
                <span class="text-muted-foreground">Updated</span>
                <span>{formatDate(db.updated_at)}</span>
              </div>
              <div class="flex items-center justify-between gap-4">
                <span class="text-muted-foreground">Engine</span>
                <span>{db.engine}</span>
              </div>
            </CardContent>
          </Card>

          <Card class="border-border/70 bg-muted/20">
            <CardHeader>
              <CardTitle class="text-base">Backups</CardTitle>
              <CardDescription>Review backup posture and manage database snapshots from the Backups tab.</CardDescription>
            </CardHeader>
            <CardContent class="flex flex-col gap-3 text-sm text-muted-foreground">
              <p>
                The backup view shows the current recovery posture, existing snapshots, and the actions available for this
                database VM.
              </p>
              <Button variant="outline" size="sm" onclick={() => (activeTab = "backups")}>
                Open Backups
              </Button>
            </CardContent>
          </Card>
        </div>
      {:else if activeTab === "connection"}
        <Card>
          <CardHeader>
            <div class="flex items-center gap-2">
              <Terminal class="size-4 text-muted-foreground" />
              <CardTitle class="text-base">Connection Details</CardTitle>
            </div>
            <CardDescription>Copy the exact commands needed to reach this database over 6PN.</CardDescription>
          </CardHeader>
          <CardContent class="flex flex-col gap-5">
            {#if connectionLoading}
              <p class="text-sm text-muted-foreground">Loading connection details...</p>
            {:else if connectionError}
              <div class="rounded-md border border-border bg-muted/30 p-4 text-sm text-muted-foreground">
                {connectionError}
              </div>
            {:else if connectionInfo}
              {@const connection = connectionInfo}
              <div class="flex flex-col gap-4">
                <div class="grid gap-3">
                  <div class="flex flex-col gap-1.5">
                    <span class="text-xs font-medium text-muted-foreground">SSH tunnel command</span>
                    <div class="rounded-md border border-border bg-muted/50 px-3 py-2 font-mono text-xs">
                      <div class="flex items-start justify-between gap-3">
                        <span class="break-all">{connection.ssh_tunnel_command}</span>
                        <Button variant="ghost" size="icon" class="size-8 shrink-0" onclick={() => copyToClipboard(connection.ssh_tunnel_command, "ssh")}>
                          {#if copiedKey === "ssh"}
                            <Check class="size-3.5 text-status-online" />
                          {:else}
                            <Copy class="size-3.5" />
                          {/if}
                        </Button>
                      </div>
                    </div>
                  </div>

                  <div class="flex flex-col gap-1.5">
                    <span class="text-xs font-medium text-muted-foreground">psql command</span>
                    <div class="rounded-md border border-border bg-muted/50 px-3 py-2 font-mono text-xs">
                      <div class="flex items-start justify-between gap-3">
                        <span class="break-all">{connection.psql_command}</span>
                        <Button variant="ghost" size="icon" class="size-8 shrink-0" onclick={() => copyToClipboard(connection.psql_command, "psql")}>
                          {#if copiedKey === "psql"}
                            <Check class="size-3.5 text-status-online" />
                          {:else}
                            <Copy class="size-3.5" />
                          {/if}
                        </Button>
                      </div>
                    </div>
                  </div>
                </div>

                <div class="grid grid-cols-2 gap-4 pt-2">
                  <div class="flex flex-col gap-1">
                    <span class="text-xs font-medium text-muted-foreground">SSH host</span>
                    <span class="font-mono text-sm">{connection.ssh_host}</span>
                  </div>
                  <div class="flex flex-col gap-1">
                    <span class="text-xs font-medium text-muted-foreground">SSH port</span>
                    <span class="font-mono text-sm">{connection.ssh_port}</span>
                  </div>
                  <div class="flex flex-col gap-1">
                    <span class="text-xs font-medium text-muted-foreground">Database user</span>
                    <span class="font-mono text-sm">{connection.database_user}</span>
                  </div>
                  <div class="flex flex-col gap-1">
                    <span class="text-xs font-medium text-muted-foreground">Database name</span>
                    <span class="font-mono text-sm">{connection.database_name}</span>
                  </div>
                </div>
              </div>
            {:else}
              <p class="text-sm text-muted-foreground">Connection information will appear once provisioning completes.</p>
            {/if}
          </CardContent>
        </Card>
      {:else if activeTab === "branches"}
        <div class="grid gap-6 lg:grid-cols-[1fr_360px]">
          <Card>
            <CardHeader>
              <div class="flex items-center gap-2">
                <Globe2 class="size-4 text-muted-foreground" />
                <CardTitle class="text-base">Current Neon Branch</CardTitle>
              </div>
              <CardDescription>
                Mikrom tracks a single branch per database. The values below are the internal Neon identifiers for that branch.
              </CardDescription>
            </CardHeader>
            <CardContent class="flex flex-col gap-5">
              {#if branchesLoading}
                <p class="text-sm text-muted-foreground">Loading branch details...</p>
              {:else if branchesError}
                <div class="rounded-md border border-border bg-muted/30 p-4 text-sm text-muted-foreground">
                  {branchesError}
                </div>
              {:else if branches[0]}
                {@const branch = branches[0]}
                <div class="flex flex-col gap-4">
                  <div class="flex items-center justify-between gap-4 rounded-md border border-border bg-background p-4">
                    <div class="flex flex-col gap-1">
                      <span class="text-xs font-medium text-muted-foreground">Branch name</span>
                      <span class="text-base font-semibold">{branch.branch_name}</span>
                    </div>
                    <Badge variant="outline" class="uppercase">{branch.status}</Badge>
                  </div>

                  <div class="grid gap-4 sm:grid-cols-2">
                    <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 p-4">
                      <span class="text-xs font-medium text-muted-foreground">Neon tenant ID</span>
                      <span class="break-all font-mono text-sm">{branch.neon_tenant_id || "Not provisioned yet"}</span>
                    </div>
                    <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 p-4">
                      <span class="text-xs font-medium text-muted-foreground">Neon timeline ID</span>
                      <span class="break-all font-mono text-sm">{branch.neon_timeline_id || "Not provisioned yet"}</span>
                    </div>
                    <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 p-4">
                      <span class="text-xs font-medium text-muted-foreground">Tenant generation</span>
                      <span class="font-mono text-sm">{branch.tenant_gen ?? "1"}</span>
                    </div>
                    <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 p-4">
                      <span class="text-xs font-medium text-muted-foreground">Current branch</span>
                      <span class="text-sm font-medium">{branch.is_current ? "Yes" : "No"}</span>
                    </div>
                  </div>
                </div>
              {:else}
                <EmptyState class="py-10">
                  <Globe2 class="size-10 text-muted-foreground" />
                  <h2 class="text-xl font-semibold">No branch data available</h2>
                  <p class="max-w-md text-sm text-muted-foreground">
                    Branch details will appear here once the database has been provisioned.
                  </p>
                </EmptyState>
              {/if}
            </CardContent>
          </Card>

          <Card class="border-border/70 bg-muted/20">
            <CardHeader>
              <CardTitle class="text-base">Branch Summary</CardTitle>
              <CardDescription>Read-only view of the Neon branch attached to this database.</CardDescription>
            </CardHeader>
            <CardContent class="flex flex-col gap-3 text-sm text-muted-foreground">
              <p>
                This view reflects the database's current Neon tenant and timeline identifiers. It does not create or delete
                branches, but it gives you the exact branch state that backs the running database.
              </p>
              <Button variant="outline" size="sm" onclick={() => (activeTab = "connection")}>
                Review connection details
              </Button>
            </CardContent>
          </Card>
        </div>
      {:else if activeTab === "backups"}
        <div class="grid gap-6 lg:grid-cols-[1fr_360px]">
          <Card>
            <CardHeader>
              <div class="flex items-center gap-2">
                <Camera class="size-4 text-muted-foreground" />
                <CardTitle class="text-base">Snapshot History</CardTitle>
              </div>
              <CardDescription>Manage VM snapshots for the active database deployment.</CardDescription>
            </CardHeader>
            <CardContent class="flex flex-col gap-5">
              {#if backupLoading}
                <p class="text-sm text-muted-foreground">Loading backup details...</p>
              {:else if backupError}
                <div class="rounded-md border border-border bg-muted/30 p-4 text-sm text-muted-foreground">
                  {backupError}
                </div>
              {:else if backupInfo}
                {@const backup = backupInfo}
                <div class="flex flex-col gap-4">
                  <div class="flex items-center justify-between gap-4 rounded-md border border-border bg-background p-4">
                    <div class="flex flex-col gap-1">
                      <span class="text-xs font-medium text-muted-foreground">Backup strategy</span>
                      <span class="text-base font-semibold capitalize">{backup.backup_strategy}</span>
                    </div>
                    <Badge variant="outline" class="uppercase">
                      {backup.retention_valid ? "Retention valid" : "Retention pending"}
                    </Badge>
                  </div>

                  <div class="grid gap-4 sm:grid-cols-2">
                    <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 p-4">
                      <span class="text-xs font-medium text-muted-foreground">Recovery mode</span>
                      <span class="text-sm">{backup.recovery_mode}</span>
                    </div>
                    <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 p-4">
                      <span class="text-xs font-medium text-muted-foreground">Retention status</span>
                      <span class="text-sm">{backup.retention_valid ? "Valid" : "Pending"}</span>
                    </div>
                    <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 p-4">
                      <span class="text-xs font-medium text-muted-foreground">Neon tenant ID</span>
                      <span class="break-all font-mono text-sm">{backup.neon_tenant_id || "Not provisioned yet"}</span>
                    </div>
                    <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 p-4">
                      <span class="text-xs font-medium text-muted-foreground">Neon timeline ID</span>
                      <span class="break-all font-mono text-sm">{backup.neon_timeline_id || "Not provisioned yet"}</span>
                    </div>
                    <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 p-4">
                      <span class="text-xs font-medium text-muted-foreground">Tenant generation</span>
                      <span class="font-mono text-sm">{backup.tenant_gen ?? "1"}</span>
                    </div>
                    <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/30 p-4">
                      <span class="text-xs font-medium text-muted-foreground">Database status</span>
                      <span class="text-sm capitalize">{backup.status}</span>
                    </div>
                  </div>

                  <div class="grid gap-4 sm:grid-cols-2">
                    <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/20 p-4">
                      <span class="text-xs font-medium text-muted-foreground">Created</span>
                      <span class="text-sm">{formatDate(backup.created_at)}</span>
                    </div>
                    <div class="flex flex-col gap-1 rounded-md border border-border bg-muted/20 p-4">
                      <span class="text-xs font-medium text-muted-foreground">Updated</span>
                      <span class="text-sm">{formatDate(backup.updated_at)}</span>
                    </div>
                  </div>
                </div>
              {:else}
                <EmptyState class="py-10">
                  <Camera class="size-10 text-muted-foreground" />
                  <h2 class="text-xl font-semibold">No backup data available</h2>
                  <p class="max-w-md text-sm text-muted-foreground">
                    Backup metadata will appear here once the database has an active Neon branch and retention has been
                    evaluated.
                  </p>
                </EmptyState>
              {/if}

              <div class="mt-2 flex items-center justify-between gap-3">
                <div class="flex flex-col gap-1">
                  <h3 class="text-sm font-semibold">Snapshots</h3>
                  <p class="text-xs text-muted-foreground">
                    Create, restore, or delete snapshots for the active database VM.
                  </p>
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  onclick={() => db && loadSnapshots(db.id)}
                  disabled={snapshotsLoading}
                >
                  Refresh
                </Button>
              </div>

              {#if snapshotsLoading}
                <p class="text-sm text-muted-foreground">Loading snapshot history...</p>
              {:else if snapshotsError}
                <div class="rounded-md border border-border bg-muted/30 p-4 text-sm text-muted-foreground">
                  {snapshotsError}
                </div>
              {:else if snapshots.length === 0}
                <EmptyState class="border border-dashed border-border py-10">
                  <Camera class="size-10 text-muted-foreground" />
                  <h2 class="text-xl font-semibold">No snapshots yet</h2>
                  <p class="max-w-md text-sm text-muted-foreground">
                    Create a snapshot from the right-hand panel to capture the current state of this database VM.
                    Snapshots become available once the deployment is active.
                  </p>
                </EmptyState>
              {:else}
                <div class="flex flex-col gap-3">
                  {#each [...snapshots].sort((a, b) => b.created_at - a.created_at) as snapshot}
                    <div class="flex flex-col gap-4 rounded-md border border-border bg-background p-4">
                      <div class="flex flex-col gap-2 md:flex-row md:items-center md:justify-between">
                        <div class="flex min-w-0 flex-col gap-1">
                          <span class="truncate font-mono text-sm font-medium">{snapshot.name}</span>
                          <span class="text-xs text-muted-foreground">
                            Created {new Date(snapshot.created_at).toLocaleString()}
                          </span>
                        </div>
                        <div class="flex flex-wrap items-center gap-2">
                          <Badge variant="outline" class="uppercase">{snapshot.vm_status.toLowerCase()}</Badge>
                          <Badge variant="outline">{formatBytes(snapshot.size_bytes)}</Badge>
                        </div>
                      </div>

                      <div class="flex flex-wrap items-center justify-between gap-3">
                        <div class="text-xs text-muted-foreground">
                          Snapshot captured from the active deployment for {backupInfo?.database_name || db.name}.
                        </div>
                        <div class="flex items-center gap-2">
                          <Button
                            variant="outline"
                            size="sm"
                            onclick={() => {
                              restoreSnapshotTarget = snapshot.name;
                              showRestoreSnapshotDialog = true;
                            }}
                            disabled={snapshotActionLoading}
                          >
                            <RotateCcw class="size-4" />
                            Restore
                          </Button>
                          <Button
                            variant="destructive-soft"
                            size="sm"
                            onclick={() => {
                              deleteSnapshotTarget = snapshot.name;
                              showDeleteSnapshotDialog = true;
                            }}
                            disabled={snapshotActionLoading}
                          >
                            <Trash2 class="size-4" />
                            Delete
                          </Button>
                        </div>
                      </div>
                    </div>
                  {/each}
                </div>
              {/if}
            </CardContent>
          </Card>

          <Card class="border-border/70 bg-muted/20">
            <CardHeader>
              <CardTitle class="text-base">Snapshot Actions</CardTitle>
              <CardDescription>Create a new snapshot or review the current recovery posture.</CardDescription>
            </CardHeader>
            <CardContent class="flex flex-col gap-4">
              <div class="flex flex-col gap-3">
                <div class="flex flex-col gap-1">
                  <span class="text-xs font-medium text-muted-foreground">Create snapshot</span>
                  <p class="text-xs text-muted-foreground">
                    Use a short descriptive name like <span class="font-mono">backup-2026-06-04</span>.
                  </p>
                  <Input
                    bind:value={snapshotName}
                    placeholder="backup-2026-06-04"
                    disabled={snapshotActionLoading}
                  />
                </div>
                {#if snapshotActionError}
                  <div class="rounded-md border border-border bg-muted/30 p-3 text-sm text-muted-foreground">
                    {snapshotActionError}
                  </div>
                {/if}
                <Button
                  class="w-full"
                  onclick={createSnapshot}
                  disabled={!snapshotName.trim() || snapshotActionLoading}
                >
                  <Camera class="size-4" />
                  Create Snapshot
                </Button>
              </div>

              <div class="rounded-md border border-border bg-background p-4 text-sm text-muted-foreground">
                <p class="font-medium text-foreground">Recovery posture</p>
                <p class="mt-1">
                  {backupInfo
                    ? backupInfo.recovery_mode
                    : "Snapshots will appear once the database deployment is active."}
                </p>
                <div class="mt-3 flex flex-wrap gap-2">
                  <Badge variant="outline" class="uppercase">
                    {backupInfo?.retention_valid ? "Retention valid" : "Retention pending"}
                  </Badge>
                  <Badge variant="outline" class="uppercase">
                    {backupInfo?.backup_strategy || "Pending"}
                  </Badge>
                </div>
              </div>

              <Button variant="outline" size="sm" onclick={() => (activeTab = "branches")}>
                Review branch details
              </Button>
            </CardContent>
          </Card>
        </div>
      {/if}

      <Card class="border-destructive/20 bg-destructive/5">
        <CardHeader>
          <div class="flex items-center gap-2 text-destructive">
            <Trash2 class="size-4" />
            <CardTitle class="text-base">Danger Zone</CardTitle>
          </div>
          <CardDescription>Remove this database from the active project.</CardDescription>
        </CardHeader>
        <CardContent class="flex flex-col gap-3">
          <p class="text-sm text-muted-foreground">
            Deleting the database removes its metadata and makes the backing VM unreachable from Mikrom.
          </p>
          <Button variant="destructive" class="w-full" onclick={() => (showDeleteDialog = true)}>
            Delete Database
          </Button>
        </CardContent>
      </Card>
    </div>
  {/if}

  <AlertDialog
    bind:open={showDeleteDialog}
    title="Are you absolutely sure?"
    description="This action cannot be undone. This will permanently delete your database and all associated data."
    actionText="Delete Database"
    variant="destructive"
    onaction={handleDelete}
  />

  <AlertDialog
    bind:open={showRestoreSnapshotDialog}
    title="Restore snapshot?"
    description={`Restore database ${db?.name || dbName} from snapshot ${restoreSnapshotTarget}. This will replace the current VM state.`}
    actionText="Restore Snapshot"
    variant="destructive"
    loading={snapshotActionLoading}
    onclose={() => (restoreSnapshotTarget = "")}
    onaction={restoreSnapshot}
  />

  <AlertDialog
    bind:open={showDeleteSnapshotDialog}
    title="Delete snapshot?"
    description={`Delete snapshot ${deleteSnapshotTarget} from database ${db?.name || dbName}? This cannot be undone.`}
    actionText="Delete Snapshot"
    variant="destructive"
    loading={snapshotActionLoading}
    onclose={() => (deleteSnapshotTarget = "")}
    onaction={deleteSnapshot}
  />
</DashboardLayout>
