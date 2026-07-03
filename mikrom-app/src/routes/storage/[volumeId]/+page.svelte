<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/stores";
  import { 
    ArrowLeft,
    HardDrive, 
    Database, 
    History, 
    Camera,
    Link,
    Activity,
    Server,
    Trash2,
    Zap
  } from "lucide-svelte";
  import {
    Card,
    CardHeader,
    CardTitle,
    CardDescription,
    CardContent,
    Button,
    Badge,
    EmptyState,
    Separator,
    Field,
    FieldGroup,
    Input,
    Modal,
    AlertDialog,
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
    SectionTabs,
  } from "$lib/components";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import { formatDate } from "$lib/utils";
  import {
    attachVolume,
    createVolumeSnapshot,
    deleteVolume,
    deleteVolumeSnapshot,
    detachVolume,
    type AttachedVolume,
    type Volume,
    type VolumeAttachmentInfo,
    type VolumeSnapshot,
    type VolumeWithAttachments,
  } from "$lib/api";
  import { getToken } from "$lib/auth";
  import { toast } from "$lib/toast";
  import { volumesStore, snapshotsStore, refreshSnapshots, refreshVolumes, snapshotsLoading } from "$lib/stores/volumes";
  import { appsStore, refreshApps } from "$lib/stores/apps";

  const volumeId = $page.params.volumeId;
  let volume: Volume | AttachedVolume | VolumeWithAttachments | undefined;
  let attachments: VolumeAttachmentInfo[];
  let showDeleteVolumeDialog = false;
  let showCreateSnapshotDialog = false;
  let showAttachVolumeDialog = false;
  let attachmentToDetach: VolumeAttachmentInfo | null = null;
  let newSnapshotName = "";
  let snapshotActionLoading = false;
  let snapshotToDelete: VolumeSnapshot | null = null;
  let attachTargetAppId = "";
  let attachMountPoint = "/data";
  let attachAccessMode = "0";

  $: volume = $volumesStore.find((v) => v.id === volumeId);
  $: attachments = volume && "attachments" in volume ? (volume.attachments as VolumeAttachmentInfo[]) : [];
  $: attachmentCount = attachments.length;
  $: isAttached = attachmentCount > 0;
  $: volumeStatusLabel = isAttached ? "Attached" : "Available";
  $: volumeUpdatedAt = "updated_at" in (volume || {}) ? (volume as AttachedVolume | VolumeWithAttachments).updated_at : volume?.created_at || "";
  $: volumePoolName = volume && "pool_name" in volume ? volume.pool_name : "ceph-rbd-ssd";

  let activeTab: "overview" | "snapshots" | "settings" = "overview";
  const volumeTabs = [
    { value: "overview", label: "Overview" },
    { value: "snapshots", label: "Snapshots" },
    { value: "settings", label: "Settings" },
  ] as const;

  onMount(async () => {
    if ($appsStore.length === 0) {
      await refreshApps();
    }
  });

  async function loadSnapshots() {
    if (!volume) return;
    await refreshSnapshots(volume.id);
  }

  async function openSnapshotsTab() {
    activeTab = "snapshots";
    await loadSnapshots();
  }

  function handleTabChange(value: string) {
    if (value === "snapshots") {
      void openSnapshotsTab();
    }
  }

  async function confirmDeleteVolume() {
    if (!volume) return;

    const token = getToken();
    if (!token) return;

    const result = await deleteVolume(token, volume.id);
    if (result.error) {
      toast.error(result.error);
      return;
    }

    toast.success("Volume deleted");
    showDeleteVolumeDialog = false;
    await goto("/storage");
  }

  async function createSnapshotNow() {
    if (!volume) return;

    const token = getToken();
    if (!token) return;

    const result = await createVolumeSnapshot(token, volume.id, { name: newSnapshotName });
    if (result.error) {
      toast.error(result.error);
      return;
    }

    toast.success("Snapshot created");
    newSnapshotName = "";
    showCreateSnapshotDialog = false;
    activeTab = "snapshots";
    await loadSnapshots();
  }

  async function deleteSnapshotNow() {
    if (!volume || !snapshotToDelete) return;

    const token = getToken();
    if (!token) {
      toast.error("You must be logged in");
      return;
    }

    snapshotActionLoading = true;
    const target = snapshotToDelete;
    const result = await deleteVolumeSnapshot(token, target.id);
    snapshotActionLoading = false;

    if (result.error) {
      toast.error(result.error);
      return;
    }

    toast.success("Snapshot deleted");
    snapshotToDelete = null;
    await loadSnapshots();
  }

  async function confirmDetachAttachment() {
    if (!volume || !attachmentToDetach) return;

    const token = getToken();
    if (!token) return;

    const target = attachmentToDetach;
    attachmentToDetach = null;

    const result = await detachVolume(token, target.app_id, volume.id);
    if (result.error) {
      toast.error(result.error);
      return;
    }

    toast.success("Volume detached");
    await refreshVolumes();
  }

  async function attachVolumeNow() {
    if (!volume || !attachTargetAppId) return;

    const token = getToken();
    if (!token) return;

    const result = await attachVolume(token, attachTargetAppId, {
      volume_id: volume.id,
      mount_point: attachMountPoint,
      access_mode: Number(attachAccessMode),
    });

    if (result.error) {
      toast.error(result.error);
      return;
    }

    toast.success("Volume attached successfully");
    showAttachVolumeDialog = false;
    attachTargetAppId = "";
    attachMountPoint = "/data";
    attachAccessMode = "0";
    await refreshVolumes();
  }
</script>

<svelte:head>
  <title>Mikrom - {volume?.name || "Volume"}</title>
</svelte:head>

<DashboardLayout>
  {#if !volume}
    <div class="flex flex-col items-center justify-center py-20">
      <EmptyState class="max-w-md">
        <HardDrive class="size-10 text-muted-foreground" />
        <h2 class="text-xl font-semibold">Volume not found</h2>
        <p class="text-sm text-muted-foreground">
          The volume you are looking for does not exist or has been deleted.
        </p>
        <Button href="/storage" variant="outline" class="mt-4">
          <ArrowLeft class="size-4" />
          Back to Storage
        </Button>
      </EmptyState>
    </div>
  {:else}
    <div class="flex flex-col gap-6">
      <div class="rounded-2xl border border-border bg-card p-5 shadow-sm md:p-6">
        <div class="flex flex-col gap-6 lg:flex-row lg:items-start lg:justify-between">
          <div class="flex min-w-0 flex-1 gap-4">
            <div class="flex size-12 shrink-0 items-center justify-center rounded-xl border border-border bg-background text-foreground">
              <HardDrive class="size-6" />
            </div>
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-3">
                <h1 class="truncate text-3xl font-semibold tracking-tight">{volume.name}</h1>
                <Badge variant={isAttached ? "secondary" : "outline"} class="uppercase">
                  {volumeStatusLabel}
                </Badge>
              </div>
              <p class="mt-2 max-w-3xl text-sm text-muted-foreground">
                Persistent block storage for application data and snapshots.
              </p>
              <div class="mt-4 flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
                <span class="inline-flex items-center gap-1.5 rounded-full border border-border bg-background px-3 py-1.5">
                  <span class="font-mono">{volume.id}</span>
                </span>
                <span class="inline-flex items-center gap-1.5 rounded-full border border-border bg-background px-3 py-1.5">
                  <span>{volume.size_mib} MiB</span>
                </span>
                <span class="inline-flex items-center gap-1.5 rounded-full border border-border bg-background px-3 py-1.5">
                  <span>Updated {formatDate(volumeUpdatedAt || volume.created_at)}</span>
                </span>
              </div>
            </div>
          </div>

          <div class="flex flex-wrap items-center gap-2">
            <Button href="/storage" variant="outline" size="sm">
              <ArrowLeft class="size-4" />
              Back
            </Button>
            <Button variant="outline" size="sm" onclick={() => (showCreateSnapshotDialog = true)}>
              <Camera class="size-4" />
              Take Snapshot
            </Button>
          </div>
        </div>

        <Separator class="my-6" />

        <div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <Card size="sm" class="border-border/70 bg-background/70 shadow-none">
            <CardContent class="flex flex-col gap-1">
              <span class="text-xs text-muted-foreground">Capacity</span>
              <span class="text-2xl font-semibold">{volume.size_mib} MiB</span>
              <span class="text-xs text-muted-foreground">Provisioned size</span>
            </CardContent>
          </Card>
          <Card size="sm" class="border-border/70 bg-background/70 shadow-none">
            <CardContent class="flex flex-col gap-1">
              <span class="text-xs text-muted-foreground">Attachments</span>
              <span class="text-2xl font-semibold">{attachmentCount}</span>
              <span class="text-xs text-muted-foreground">Applications using this volume</span>
            </CardContent>
          </Card>
          <Card size="sm" class="border-border/70 bg-background/70 shadow-none">
            <CardContent class="flex flex-col gap-1">
              <span class="text-xs text-muted-foreground">Storage pool</span>
              <span class="text-xl font-semibold">{volumePoolName}</span>
              <span class="text-xs text-muted-foreground">Ceph RBD backend</span>
            </CardContent>
          </Card>
          <Card size="sm" class="border-border/70 bg-background/70 shadow-none">
            <CardContent class="flex flex-col gap-1">
              <span class="text-xs text-muted-foreground">Last updated</span>
              <span class="text-xl font-semibold">{formatDate(volumeUpdatedAt || volume.created_at)}</span>
              <span class="text-xs text-muted-foreground">Activity and metadata changes</span>
            </CardContent>
          </Card>
        </div>
      </div>

      <SectionTabs bind:active={activeTab} tabs={volumeTabs} onChange={handleTabChange} />

      {#if activeTab === "overview"}
        <div class="grid gap-6 md:grid-cols-3">
          <Card class="overflow-hidden md:col-span-2">
            <CardHeader>
              <CardTitle>Volume Details</CardTitle>
              <CardDescription>General information about this persistent volume.</CardDescription>
            </CardHeader>
            <CardContent class="grid gap-6">
              <div class="grid gap-4 sm:grid-cols-2">
                <div class="rounded-2xl border border-border/70 bg-background/60 p-4">
                  <p class="text-xs text-muted-foreground">Status</p>
                  <div class="mt-2 flex items-center gap-2">
                    <span class="size-2 rounded-full bg-status-online"></span>
                    <span class="text-sm font-medium">{volumeStatusLabel}</span>
                  </div>
                </div>
                <div class="rounded-2xl border border-border/70 bg-background/60 p-4">
                  <p class="text-xs text-muted-foreground">Type</p>
                  <div class="mt-2 flex items-center gap-2 text-sm font-medium">
                    <Zap class="size-4 text-amber-500" />
                    Ceph RBD
                  </div>
                </div>
                <div class="rounded-2xl border border-border/70 bg-background/60 p-4">
                  <p class="text-xs text-muted-foreground">Replication</p>
                  <p class="mt-2 text-sm font-medium">3x replicated</p>
                </div>
                <div class="rounded-2xl border border-border/70 bg-background/60 p-4">
                  <p class="text-xs text-muted-foreground">Updated</p>
                  <p class="mt-2 text-sm font-medium">{formatDate(volumeUpdatedAt || volume.created_at)}</p>
                </div>
              </div>

              <Separator />

              <div class="flex items-center justify-between gap-3">
                <h3 class="text-sm font-semibold">Current Attachments</h3>
                <Badge variant="outline">{attachmentCount} total</Badge>
              </div>
              <div class="grid gap-3">
                <div class="grid gap-3">
                  {#if attachments.length > 0}
                    {#each attachments as attachment}
                      <div class="flex items-center justify-between rounded-2xl border border-border/70 bg-background/70 p-4">
                        <div class="flex items-center gap-3">
                          <div class="flex size-9 items-center justify-center rounded-lg border border-border bg-background">
                            <Server class="size-4" />
                          </div>
                        <div class="flex flex-col">
                            <span class="text-sm font-medium">{attachment.app_name}</span>
                            <span class="text-xs text-muted-foreground">{attachment.mount_point} ({attachment.access_mode === 1 ? "RWX" : "RWO"})</span>
                          </div>
                        </div>
                        <Button
                          variant="ghost"
                          size="sm"
                          class="text-destructive hover:bg-destructive/10"
                          onclick={() => (attachmentToDetach = attachment)}
                        >
                          Detach
                        </Button>
                      </div>
                    {/each}
                  {:else}
                    <EmptyState class="border border-dashed border-border py-10">
                      <Link class="mb-2 size-6 text-muted-foreground/50" />
                      <p class="text-xs text-muted-foreground">
                        This volume is not attached to any application.
                      </p>
                      <Button
                        variant="outline"
                        size="sm"
                        class="mt-3"
                        onclick={() => (showAttachVolumeDialog = true)}
                      >
                        Attach to Application
                      </Button>
                    </EmptyState>
                  {/if}
                </div>
              </div>
            </CardContent>
          </Card>

          <div class="flex flex-col gap-6">
            <Card class="overflow-hidden">
              <CardHeader>
                <CardTitle class="text-base">Storage Pool</CardTitle>
              </CardHeader>
              <CardContent>
                <div class="flex items-center gap-3">
                  <div class="flex size-10 items-center justify-center rounded-full bg-primary/10 text-primary">
                    <Database class="size-5" />
                  </div>
                  <div class="flex flex-col">
                    <span class="text-sm font-medium">ceph-rbd-ssd</span>
                    <span class="text-xs text-muted-foreground">High Performance</span>
                  </div>
                </div>
              </CardContent>
            </Card>

            <Card class="overflow-hidden">
              <CardHeader>
                <CardTitle class="text-base">Usage Stats</CardTitle>
              </CardHeader>
              <CardContent class="grid gap-4">
                <div class="flex flex-col gap-2">
                  <div class="flex items-center justify-between text-xs">
                    <span class="text-muted-foreground">Provisioned</span>
                    <span class="font-medium">{volume.size_mib} MiB</span>
                  </div>
                  <div class="h-2 w-full rounded-full bg-muted">
                    <div class="h-full w-[45%] rounded-full bg-primary"></div>
                  </div>
                  <div class="flex items-center justify-between text-[10px] text-muted-foreground">
                    <span>Used: ~460 MiB</span>
                    <span>Free: ~564 MiB</span>
                  </div>
                </div>
                <div class="flex items-center gap-2 text-xs text-muted-foreground">
                  <Activity class="size-3" />
                  <span>I/O is healthy</span>
                </div>
              </CardContent>
            </Card>
          </div>
        </div>
      {:else if activeTab === "snapshots"}
        <Card class="overflow-hidden">
          <CardHeader class="flex flex-row items-center justify-between gap-3">
            <div class="flex flex-col gap-1">
              <CardTitle class="text-base">Snapshot History</CardTitle>
              <CardDescription>Recover data or clone this volume from a point in time.</CardDescription>
            </div>
            <Button variant="outline" size="sm" onclick={() => (showCreateSnapshotDialog = true)}>
              <Camera class="mr-2 size-4" />
              Create Snapshot
            </Button>
          </CardHeader>
          <CardContent>
            {#if $snapshotsLoading}
              <div class="rounded-md border border-dashed border-border bg-muted/10 p-4 text-sm text-muted-foreground">
                Loading snapshots...
              </div>
            {:else if $snapshotsStore.length === 0}
              <EmptyState class="py-12">
                <History class="mb-4 size-10 text-muted-foreground/30" />
                <p class="text-sm text-muted-foreground">No snapshots available for this volume yet.</p>
                <Button variant="outline" class="mt-4" onclick={() => (showCreateSnapshotDialog = true)}>
                  <Camera class="mr-2 size-4" />
                  Create first snapshot
                </Button>
              </EmptyState>
            {:else}
              <div class="flex flex-col gap-4">
                {#each $snapshotsStore as snapshot}
                  <div class="flex items-center justify-between rounded-2xl border border-border/70 bg-background/70 p-4">
                    <div class="flex items-center gap-3">
                      <div class="flex size-9 items-center justify-center rounded-lg border border-border bg-background">
                        <Camera class="size-4" />
                      </div>
                      <div class="flex flex-col">
                        <span class="text-sm font-medium">{snapshot.name}</span>
                        <span class="text-xs text-muted-foreground">{formatDate(snapshot.created_at)}</span>
                      </div>
                    </div>
                    <Button
                      variant="destructive-soft"
                      size="icon"
                      class="size-8 shrink-0"
                      title="Delete snapshot"
                      onclick={() => (snapshotToDelete = snapshot)}
                      disabled={snapshotActionLoading}
                    >
                      <Trash2 class="size-4" />
                    </Button>
                  </div>
                {/each}
              </div>
            {/if}
          </CardContent>
        </Card>
      {:else if activeTab === "settings"}
        <Card class="overflow-hidden">
          <CardHeader>
            <CardTitle class="text-base">Volume Settings</CardTitle>
            <CardDescription>Configure volume parameters and management.</CardDescription>
          </CardHeader>
          <CardContent class="grid gap-6">
            <FieldGroup>
              <Field label="Volume Name" forId="vol-name" description="Current user-facing name.">
                <Input id="vol-name" value={volume.name} readonly />
              </Field>
            </FieldGroup>

            <Separator />

            <Card size="sm" class="border-destructive/20 bg-destructive/5 shadow-none">
              <CardContent class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                <div class="flex flex-col gap-0.5">
                  <span class="text-sm font-semibold text-destructive">Danger Zone</span>
                  <span class="text-xs text-muted-foreground">Irreversible actions for this volume.</span>
                </div>
                <Button variant="destructive" size="sm" onclick={() => (showDeleteVolumeDialog = true)}>
                  Delete Volume
                </Button>
              </CardContent>
            </Card>
          </CardContent>
        </Card>
      {/if}
    </div>
  {/if}

  {#if showCreateSnapshotDialog}
    <Modal
      bind:open={showCreateSnapshotDialog}
      title="Create snapshot"
      description={`Create a point-in-time copy for volume ${volume?.name || ""}.`}
    >
      <FieldGroup class="pt-2">
        <Field label="Snapshot name">
          <Input bind:value={newSnapshotName} placeholder="daily-backup" />
        </Field>
      </FieldGroup>
      <div class="mt-6 flex justify-end gap-3">
        <Button variant="outline" onclick={() => (showCreateSnapshotDialog = false)}>Cancel</Button>
        <Button onclick={createSnapshotNow} disabled={!newSnapshotName}>Create Snapshot</Button>
      </div>
    </Modal>
  {/if}

  {#if showAttachVolumeDialog}
    <Modal
      bind:open={showAttachVolumeDialog}
      title="Attach volume to application"
      description={`Attach volume ${volume?.name || ""} to an application.`}
    >
      <FieldGroup class="pt-2">
        <Field label="Select Application">
          <Select bind:value={attachTargetAppId}>
            <SelectTrigger>
              <SelectValue placeholder="Choose an app..." />
            </SelectTrigger>
            <SelectContent>
              {#each $appsStore as app}
                <SelectItem value={app.id}>{app.name}</SelectItem>
              {/each}
            </SelectContent>
          </Select>
        </Field>
        <Field label="Mount point"><Input bind:value={attachMountPoint} placeholder="/data" /></Field>
        <Field label="Access Mode">
          <Select bind:value={attachAccessMode}>
            <SelectTrigger>
              <SelectValue placeholder="Select access mode" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="0">RWO - ReadWriteOnce (Single Node)</SelectItem>
              <SelectItem value="1">RWX - ReadWriteMany (Shared Mesh)</SelectItem>
              <SelectItem value="2">ROX - ReadOnlyMany (Shared Read)</SelectItem>
            </SelectContent>
          </Select>
        </Field>
      </FieldGroup>
      <div class="mt-6 flex justify-end gap-3">
        <Button
          variant="outline"
          onclick={() => {
            showAttachVolumeDialog = false;
            attachTargetAppId = "";
            attachMountPoint = "/data";
            attachAccessMode = "0";
          }}
        >
          Cancel
        </Button>
        <Button onclick={attachVolumeNow} disabled={!attachTargetAppId}>Attach Volume</Button>
      </div>
    </Modal>
  {/if}

  <AlertDialog
    open={!!attachmentToDetach}
    title="Detach volume?"
    description={`Detach volume ${volume?.name} from ${attachmentToDetach?.app_name}?`}
    actionText="Detach"
    variant="destructive"
    onaction={confirmDetachAttachment}
    onclose={() => (attachmentToDetach = null)}
  />

  <AlertDialog
    open={!!snapshotToDelete}
    title="Delete snapshot?"
    description={`Delete snapshot ${snapshotToDelete?.name} from volume ${volume?.name}? This cannot be undone.`}
    actionText="Delete Snapshot"
    variant="destructive"
    loading={snapshotActionLoading}
    onaction={deleteSnapshotNow}
    onclose={() => (snapshotToDelete = null)}
  />

  <AlertDialog
    bind:open={showDeleteVolumeDialog}
    title="Delete volume?"
    description={`Are you sure you want to delete volume ${volume?.name}? All data and snapshots will be permanently lost.`}
    actionText="Delete Volume"
    variant="destructive"
    onaction={confirmDeleteVolume}
    onclose={() => (showDeleteVolumeDialog = false)}
  />
</DashboardLayout>
