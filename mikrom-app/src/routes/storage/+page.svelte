<script lang="ts">
  import { onMount } from "svelte";
  import { Calendar, Camera, Copy, HardDrive, Plus, RotateCcw, Trash2 } from "lucide-svelte";
  import {
    Card,
    CardHeader,
    CardTitle,
    CardContent,
    CardSkeleton,
    Button,
    AlertDialog,
    EmptyState,
    TableSkeleton,
    Table,
    TableHeader,
    TableBody,
    TableRow,
    TableHead,
    TableCell,
    Modal,
    Field,
    FieldGroup,
    Input,
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
    Badge,
  } from "$lib/components";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import { formatDate } from "$lib/utils";
  import { matchesSearch } from "$lib/search";

  import { getToken } from "$lib/auth";
  import {
    createVolume,
    attachVolume,
    cloneVolumeFromSnapshot,
    deleteVolumeSnapshot,
    restoreVolumeSnapshot,
    type Volume,
    type VolumeSnapshot,
  } from "$lib/api";
  import { toast } from "$lib/toast";
  import { appsStore, refreshApps } from "$lib/stores/apps";
  import { volumesStore, snapshotsStore, volumesLoading, snapshotsLoading, refreshVolumes, refreshSnapshots } from "$lib/stores/volumes";
  import type { AttachedVolume, VolumeWithAttachments } from "$lib/api";

  let selectedApp = "";
  let _selectedAppId = "";
  let showCreateVolume = false;
  let showAttachVolume = false;
  let showCloneModal = false;
  let query = "";
  let volumeForSnapshots = "";
  let snapshotToDelete: VolumeSnapshot | null = null;
  let showSnapshotsModal = false;
  let restoreSnapshotName = "";
  let snapshotToClone = "";
  let cloneName = "";
  let newVolume = { name: "", size_mib: 1024 };
  
  let attachParams = { 
    volume_id: "", 
    app_id: "", 
    mount_point: "/data", 
    access_mode: "0" 
  };

  async function loadVolumes(appName: string) {
    if (!appName) {
      _selectedAppId = "";
      await refreshVolumes();
      return;
    }
    const app = $appsStore.find((item) => item.name === appName);
    if (!app) {
      _selectedAppId = "";
      return;
    }
    _selectedAppId = app.id;
    await refreshVolumes(app.id);
  }

  async function _loadSnapshots(volume_id: string) {
    await refreshSnapshots(volume_id);
  }

  onMount(async () => {
    if ($appsStore.length === 0) {
      await refreshApps();
    }
    // Load all volumes by default
    await refreshVolumes();
  });

  async function createNewVolume() {
    const token = getToken();
    if (!token) return;
    const result = await createVolume(token, newVolume);
    if (result.error) return toast.error(result.error);
    toast.success("Volume created successfully");
    newVolume = { name: "", size_mib: 1024 };
    showCreateVolume = false;
    // Refresh the list immediately
    await loadVolumes(selectedApp);
  }

  async function attachVolumeNow() {
    const token = getToken();
    if (!token || !attachParams.volume_id || !attachParams.app_id) return;

    const result = await attachVolume(token, attachParams.app_id, {
      volume_id: attachParams.volume_id,
      mount_point: attachParams.mount_point,
      access_mode: Number(attachParams.access_mode)
    });

    if (result.error) return toast.error(result.error);
    toast.success("Volume attached successfully");
    showAttachVolume = false;
    await loadVolumes(selectedApp);
  }

  async function deleteSnapshotNow(id: string) {
    const token = getToken();
    if (!token) return;
    const result = await deleteVolumeSnapshot(token, id);
    if (result.error) return toast.error(result.error);
    toast.success("Snapshot deleted");
    await _loadSnapshots(volumeForSnapshots);
  }

  async function confirmDeleteSnapshot() {
    if (!snapshotToDelete) return;
    const target = snapshotToDelete;
    snapshotToDelete = null;
    await deleteSnapshotNow(target.id);
  }

  async function restoreSnapshot() {
    const token = getToken();
    if (!token) return;
    const result = await restoreVolumeSnapshot(token, volumeForSnapshots, { snapshot_name: restoreSnapshotName });
    if (result.error) return toast.error(result.error);
    toast.success("Volume restored to snapshot");
    restoreSnapshotName = "";
  }

  async function cloneSnapshot() {
    const token = getToken();
    if (!token) return;
    const result = await cloneVolumeFromSnapshot(token, snapshotToClone, { name: cloneName, snapshot_name: restoreSnapshotName });
    if (result.error) return toast.error(result.error);
    toast.success("Volume cloned from snapshot");
    snapshotToClone = "";
    cloneName = "";
    restoreSnapshotName = "";
    showCloneModal = false;
    await loadVolumes(selectedApp);
  }

  function closeCloneModal() {
    showCloneModal = false;
    snapshotToClone = "";
    cloneName = "";
    restoreSnapshotName = "";
  }

  function formatSize(size: number) {
    return size >= 1024 ? `${(size / 1024).toFixed(1)} GiB` : `${size} MiB`;
  }

  function getVolumeSearchValues(volume: Volume | AttachedVolume | VolumeWithAttachments) {
    const values = [volume.name];

    if ("pool_name" in volume) {
      values.push(volume.pool_name);
    }

    if ("attachments" in volume) {
      values.push(
        ...volume.attachments.flatMap((attachment) => [attachment.app_name, attachment.mount_point])
      );
    }

    return values;
  }

  $: filteredVolumes = [...$volumesStore]
    .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
    .filter((volume) => matchesSearch(getVolumeSearchValues(volume), query));
</script>

<svelte:head>
  <title>Mikrom - Storage</title>
</svelte:head>

<DashboardLayout>
  <div class="flex flex-col gap-6">
    <div class="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
      <div class="flex flex-col gap-2">
        <div class="flex items-center gap-3">
          <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
            <HardDrive />
          </div>
          <h1 class="text-3xl font-semibold tracking-tight">Storage</h1>
        </div>
        <p class="max-w-2xl text-sm text-muted-foreground">Manage persistent block storage (Ceph RBD) for your applications.</p>
      </div>
      <Button onclick={() => (showCreateVolume = true)}>
        <Plus class="size-4" />
        New Volume
      </Button>
    </div>

    <Card size="sm" class="overflow-hidden">
      <CardContent class="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <div class="min-w-0 flex-1">
          <Input bind:value={query} placeholder="Search by volume name, app or mount point" />
        </div>
      </CardContent>
    </Card>

    <div class="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
      {#if $volumesLoading && $volumesStore.length === 0}
        {#each Array.from({ length: 6 }) as _}
          <CardSkeleton
            titleClassName="w-32"
            descriptionClassName="w-full"
            footerLineClassName="w-40"
            footerPills={["w-20", "w-24"]}
          />
        {/each}
      {:else if filteredVolumes.length === 0}
        <div class="col-span-full">
          <EmptyState class="py-16">
            <HardDrive class="size-10 text-muted-foreground" />
            <h2 class="text-xl font-semibold">{query ? "No matching volumes" : "No volumes found"}</h2>
            <p class="max-w-md text-sm text-muted-foreground">
              {query
                ? "Try another search term or clear the search box."
                : "You don't have any persistent volumes yet."}
            </p>
            <Button size="sm" onclick={() => (showCreateVolume = true)}>
              <Plus class="size-4" />
              Create your first volume
            </Button>
          </EmptyState>
        </div>
      {:else}
        {#each filteredVolumes as volume (volume.id)}
          <a href={`/storage/${volume.id}`} class="block">
            <Card size="sm" class="overflow-hidden">
              <CardHeader>
                <div class="flex items-start gap-4">
                  <div class="flex size-10 shrink-0 items-center justify-center rounded-md border border-border bg-background text-foreground">
                    <HardDrive class="size-5" />
                  </div>
                  <div class="flex min-w-0 flex-1 flex-col gap-2">
                    <CardTitle class="truncate text-base">{volume.name}</CardTitle>
                  </div>
                </div>
              </CardHeader>
              <CardContent class="flex flex-col gap-4">
                <div class="flex flex-col gap-3 text-xs text-muted-foreground">
                  <span class="inline-flex items-center gap-1.5">
                    <Calendar class="size-4" />
                    Created {formatDate(volume.created_at)}
                  </span>
                  <span class="inline-flex items-center gap-1.5">
                    <Calendar class="size-4" />
                    Updated {formatDate(volume.updated_at)}
                  </span>
                  <div class="flex flex-wrap items-center gap-2">
                    <Badge variant="outline" class="gap-1.5">
                      <HardDrive class="size-3" />
                      <span>{formatSize(volume.size_mib)}</span>
                    </Badge>
                  </div>
                </div>
              </CardContent>
            </Card>
          </a>
        {/each}
      {/if}
    </div>
  </div>

  {#if showCreateVolume}
    <Modal bind:open={showCreateVolume} title="Create new volume" description="The volume will be available to be attached to any application.">
      <FieldGroup class="pt-2">
        <Field label="Volume name"><Input bind:value={newVolume.name} placeholder="my-data-volume" /></Field>
        <Field label="Size (MiB)"><Input type="number" bind:value={newVolume.size_mib} min={128} /></Field>
      </FieldGroup>
      <div class="mt-6 flex justify-end gap-3">
        <Button variant="outline" onclick={() => (showCreateVolume = false)}>Cancel</Button>
        <Button onclick={createNewVolume}>Create Volume</Button>
      </div>
    </Modal>
  {/if}

  {#if showAttachVolume}
    <Modal bind:open={showAttachVolume} title="Attach volume to application" description="Configure how the volume should be mounted in the microVM.">
      <FieldGroup class="pt-2">
        <Field label="Select Application">
          <Select bind:value={attachParams.app_id}>
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
        <Field label="Mount point"><Input bind:value={attachParams.mount_point} placeholder="/data" /></Field>
        <Field label="Access Mode">
          <Select bind:value={attachParams.access_mode}>
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
        <Button variant="outline" onclick={() => (showAttachVolume = false)}>Cancel</Button>
        <Button onclick={attachVolumeNow}>Attach Volume</Button>
      </div>
    </Modal>
  {/if}

  <Modal open={showSnapshotsModal} title="Snapshot history" width="max-w-3xl" description={`Manage snapshots for volume ${$volumesStore.find((v) => v.id === volumeForSnapshots)?.name || ""}.`} onclose={() => { showSnapshotsModal = false; volumeForSnapshots = ""; snapshotsStore.set([]); }}>
    <div class="mt-4">
      {#if $snapshotsLoading}
        <div class="rounded-md border border-dashed border-border bg-muted/10 p-4 text-sm text-muted-foreground">
          Loading snapshots...
        </div>
      {:else if $snapshotsStore.length === 0}
        <EmptyState class="py-8">
          <Camera class="size-8 text-muted-foreground" />
          <p class="text-sm text-muted-foreground">No snapshots yet for this volume.</p>
          <Button variant="outline" class="mt-4" href={volumeForSnapshots ? `/storage/${encodeURIComponent(volumeForSnapshots)}` : "/storage"}>
            <Camera class="mr-2 size-4" />
            Open volume details
          </Button>
        </EmptyState>
      {:else}
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Snapshot Name</TableHead>
              <TableHead>Created</TableHead>
              <TableHead class="text-right">Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {#each $snapshotsStore as snap}
              <TableRow>
                <TableCell class="font-mono text-xs">{snap.name}</TableCell>
                <TableCell class="text-muted-foreground text-xs">{new Date(snap.created_at).toLocaleString()}</TableCell>
                <TableCell class="text-right">
                  <div class="flex justify-end gap-1">
                    <Button variant="outline" size="icon" class="size-7" title="Restore" onclick={() => { restoreSnapshotName = snap.name; restoreSnapshot(); }}>
                      <RotateCcw class="size-3.5" />
                    </Button>
                    <Button variant="outline" size="icon" class="size-7" title="Clone" onclick={() => { snapshotToClone = snap.volume_id; restoreSnapshotName = snap.name; cloneName = `${$volumesStore.find((v) => v.id === snap.volume_id)?.name || "volume"}-clone`; showCloneModal = true; }}>
                      <Copy class="size-3.5" />
                    </Button>
                    <Button variant="destructive-soft" size="icon" class="size-7" title="Delete" onclick={() => (snapshotToDelete = snap)}>
                      <Trash2 class="size-3.5" />
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            {/each}
          </TableBody>
        </Table>
      {/if}
    </div>
  </Modal>

  <Modal
    bind:open={showCloneModal}
    title="Clone snapshot"
    description={`Create a new volume from snapshot ${restoreSnapshotName}.`}
    onclose={closeCloneModal}
  >
    <FieldGroup class="pt-2">
      <Field label="New volume name">
        <Input bind:value={cloneName} placeholder="my-volume-clone" />
      </Field>
    </FieldGroup>
    <div class="mt-6 flex justify-end gap-3">
      <Button variant="outline" onclick={closeCloneModal}>Cancel</Button>
      <Button onclick={cloneSnapshot} disabled={!snapshotToClone || !cloneName}>Clone Snapshot</Button>
    </div>
  </Modal>

  <AlertDialog
    open={!!snapshotToDelete}
    title="Delete snapshot?"
    description={`Are you sure you want to delete snapshot ${snapshotToDelete?.name}?`}
    actionText="Delete Snapshot"
    variant="destructive"
    onclose={() => (snapshotToDelete = null)}
    onaction={confirmDeleteSnapshot}
  />
</DashboardLayout>
