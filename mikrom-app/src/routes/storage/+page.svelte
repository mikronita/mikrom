<script lang="ts">
  import { onMount } from "svelte";
  import { Camera, Copy, Database, HardDrive, History, Link, Plus, RotateCcw, Trash2 } from "lucide-svelte";
  import {
    Card,
    CardHeader,
    CardTitle,
    CardDescription,
    CardContent,
    Button,
    AlertDialog,
    EmptyState,
    Skeleton,
    TableSkeleton,
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
    createVolume, 
    attachVolume, 
    detachVolume,
    createVolumeSnapshot, 
    cloneVolumeFromSnapshot, 
    deleteVolume, 
    deleteVolumeSnapshot, 
    restoreVolumeSnapshot, 
    type Volume, 
    type VolumeSnapshot, 
    type AttachedVolume,
    type VolumeWithAttachments
  } from "$lib/api";
  import { toast } from "$lib/toast";
  import { appsStore, refreshApps } from "$lib/stores/apps";
  import { volumesStore, snapshotsStore, volumesLoading, snapshotsLoading, refreshVolumes, refreshSnapshots } from "$lib/stores/volumes";

  let selectedApp = "";
  let selectedAppId = "";
  let showCreateVolume = false;
  let showAttachVolume = false;
  let showCloneModal = false;
  let volumeForSnapshots = "";
  let volumeToDelete: Volume | null = null;
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
    access_mode: 0 
  };

  async function loadVolumes(appName: string) {
    if (!appName) {
      selectedAppId = "";
      await refreshVolumes();
      return;
    }
    const app = $appsStore.find((item) => item.name === appName);
    if (!app) {
      selectedAppId = "";
      return;
    }
    selectedAppId = app.id;
    await refreshVolumes(app.id);
  }

  async function loadSnapshots(volume_id: string) {
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
      access_mode: attachParams.access_mode
    });
    
    if (result.error) return toast.error(result.error);
    toast.success("Volume attached successfully");
    showAttachVolume = false;
    await loadVolumes(selectedApp);
  }

  async function detachVolumeNow(appId: string, volumeId: string) {
    const token = getToken();
    if (!token) return;
    const result = await detachVolume(token, appId, volumeId);
    if (result.error) return toast.error(result.error);
    toast.success("Volume detached");
    await loadVolumes(selectedApp);
  }

  async function createSnapshot(volumeId: string) {
    const token = getToken();
    if (!token) return;
    const snapName = `snap-${new Date().toISOString().replace(/[:.]/g, "-")}`;
    const result = await createVolumeSnapshot(token, volumeId, { name: snapName });
    if (result.error) return toast.error(result.error);
    toast.success("Snapshot created");
    await loadSnapshots(volumeId);
  }

  async function deleteVolumeNow(id: string) {
    const token = getToken();
    if (!token) return;
    const result = await deleteVolume(token, id);
    if (result.error) return toast.error(result.error);
    toast.success("Volume deleted");
    await loadVolumes(selectedApp);
  }

  async function deleteSnapshotNow(id: string) {
    const token = getToken();
    if (!token) return;
    const result = await deleteVolumeSnapshot(token, id);
    if (result.error) return toast.error(result.error);
    toast.success("Snapshot deleted");
    await loadSnapshots(volumeForSnapshots);
  }

  async function confirmDeleteVolume() {
    if (!volumeToDelete) return;
    const target = volumeToDelete;
    volumeToDelete = null;
    await deleteVolumeNow(target.id);
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

  function isAttachedVolume(v: Volume | AttachedVolume | VolumeWithAttachments): v is AttachedVolume {
    return "mount_point" in v;
  }

  function isVolumeWithAttachments(v: Volume | AttachedVolume | VolumeWithAttachments): v is VolumeWithAttachments {
    return "attachments" in v;
  }
</script>

<svelte:head>
  <title>Mikrom - Storage</title>
</svelte:head>

<DashboardLayout>
  <div class="flex flex-col gap-6">
    <div class="flex flex-col gap-2">
      <div class="flex items-center gap-3">
        <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
          <HardDrive />
        </div>
        <h1 class="text-3xl font-semibold tracking-tight">Storage</h1>
      </div>
      <p class="max-w-2xl text-sm text-muted-foreground">Manage persistent block storage (Ceph RBD) for your applications.</p>
    </div>

    <Card class="overflow-hidden">
      <CardHeader class="border-b bg-muted/20">
        <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
          <div class="flex flex-col gap-1.5">
            <CardTitle class="flex items-center gap-2 text-lg">
              <Database class="size-5" />
              Volumes
            </CardTitle>
            <CardDescription>
              Persistent volumes can be attached to your microVMs.
            </CardDescription>
          </div>
          <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
            <Select bind:value={selectedApp} onValueChange={async (val: string | undefined) => {
              selectedApp = val || "";
              await loadVolumes(selectedApp);
            }}>
              <SelectTrigger class="sm:w-[220px]">
                <SelectValue placeholder="All Applications" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="">All Applications</SelectItem>
                {#each $appsStore as app}
                  <SelectItem value={app.name}>{app.name}</SelectItem>
                {/each}
              </SelectContent>
            </Select>
            <Button size="sm" onclick={() => (showCreateVolume = true)}>
              <Plus class="size-4" />
              Create Volume
            </Button>
          </div>
        </div>
      </CardHeader>

      <CardContent class="p-0">
        {#if $volumesLoading}
          <div class="flex flex-col gap-3 p-4">
            <Skeleton class="h-10 w-full" />
            <Skeleton class="h-10 w-full" />
          </div>
        {:else if $volumesStore.length === 0}
          <EmptyState><HardDrive class="size-10 text-muted-foreground" /><h3 class="text-xl font-semibold">No volumes found</h3><p class="text-sm text-muted-foreground">You don't have any persistent volumes yet.</p></EmptyState>
        {:else}
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Volume</TableHead>
                <TableHead>Attached To</TableHead>
                <TableHead class="text-center w-[120px]">Size</TableHead>
                <TableHead class="text-right w-[180px]">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {#each $volumesStore as volume (volume.id)}
                <TableRow class="group">
                  <TableCell class="font-medium">
                    <div class="flex items-center gap-3">
                      <div class="flex size-8 shrink-0 items-center justify-center rounded-md border border-border bg-muted/30 text-muted-foreground group-hover:bg-background transition-colors">
                        <Database class="size-4" />
                      </div>
                      <div class="flex flex-col min-w-0">
                        <span class="truncate font-semibold text-sm">{volume.name}</span>
                        <span class="font-mono text-[10px] text-muted-foreground/70 lowercase tracking-wider">{volume.id}</span>
                      </div>
                    </div>
                  </TableCell>
                  
                  <TableCell>
                    <div class="flex flex-wrap gap-2">
                      {#if isAttachedVolume(volume)}
                        <Badge variant="outline" class="h-7 items-center gap-1.5 px-2 group/badge bg-muted/20 border-border/50">
                          <span class="font-medium text-foreground">{$appsStore.find((a) => a.id === selectedAppId)?.name || selectedApp || "Current App"}</span>
                          <Badge variant="secondary" class="h-4 px-1 text-[8px] font-bold uppercase tracking-tighter">
                            {volume.access_mode === 1 ? "RWX" : volume.access_mode === 2 ? "ROX" : "RWO"}
                          </Badge>
                          <code class="text-[9px] text-muted-foreground bg-background/50 px-1 rounded border border-border/30">{volume.mount_point}</code>
                          <button 
                            class="ml-0.5 rounded-full p-0.5 hover:bg-destructive/10 hover:text-destructive transition-colors text-muted-foreground" 
                            title="Detach"
                            onclick={() => detachVolumeNow(selectedAppId, volume.id)}
                          >
                            <Plus class="size-3 rotate-45" />
                          </button>
                        </Badge>
                      {:else if isVolumeWithAttachments(volume)}
                        {#if volume.attachments.length === 0}
                          <span class="text-xs text-muted-foreground italic opacity-50">Not attached</span>
                        {:else}
                          {#each volume.attachments as attachment}
                            <Badge variant="outline" class="h-7 items-center gap-1.5 px-2 group/badge bg-muted/20 border-border/50">
                              <span class="font-medium text-foreground">{attachment.app_name}</span>
                              <Badge variant="secondary" class="h-4 px-1 text-[8px] font-bold uppercase tracking-tighter">
                                {attachment.access_mode === 1 ? "RWX" : attachment.access_mode === 2 ? "ROX" : "RWO"}
                              </Badge>
                              <code class="text-[9px] text-muted-foreground bg-background/50 px-1 rounded border border-border/30">{attachment.mount_point}</code>
                              <button 
                                class="ml-0.5 rounded-full p-0.5 hover:bg-destructive/10 hover:text-destructive transition-colors text-muted-foreground" 
                                title={`Detach from ${attachment.app_name}`}
                                onclick={() => detachVolumeNow(attachment.app_id, volume.id)}
                              >
                                <Plus class="size-3 rotate-45" />
                              </button>
                            </Badge>
                          {/each}
                        {/if}
                      {/if}
                    </div>
                  </TableCell>

                  <TableCell class="text-center">
                    <Badge variant="secondary" class="font-mono h-6">{formatSize(volume.size_mib)}</Badge>
                  </TableCell>
                  
                  <TableCell class="text-right">
                    <div class="flex justify-end gap-1">
                      {#if !selectedApp}
                        <Button variant="ghost" size="icon" class="size-8" title="Attach to App" onclick={() => { attachParams.volume_id = volume.id; showAttachVolume = true; }}>
                          <Link class="size-4" />
                        </Button>
                      {/if}
                      <Button variant="ghost" size="icon" class="size-8" title="Snapshots" onclick={async () => { volumeForSnapshots = volume.id; showSnapshotsModal = true; await loadSnapshots(volume.id); }}>
                        <History class="size-4" />
                      </Button>
                      <Button variant="ghost" size="icon" class="size-8" title="Take Snapshot" onclick={() => createSnapshot(volume.id)}>
                        <Camera class="size-4" />
                      </Button>
                      <Button variant="destructive-soft" size="icon" class="size-8" title="Delete Volume" onclick={() => (volumeToDelete = volume)}>
                        <Trash2 class="size-4" />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              {/each}
            </TableBody>
          </Table>
        {/if}
      </CardContent>
    </Card>
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
        <TableSkeleton rows={3} cols={3} />
      {:else if $snapshotsStore.length === 0}
        <EmptyState class="py-8"><Camera class="size-8 text-muted-foreground" /><p class="text-sm text-muted-foreground">No snapshots yet.</p></EmptyState>
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
    open={!!volumeToDelete}
    title="Delete volume?"
    description={`Are you sure you want to delete volume ${volumeToDelete?.name}? All data and snapshots will be permanently lost.`}
    actionText="Delete Volume"
    variant="destructive"
    onclose={() => (volumeToDelete = null)}
    onaction={confirmDeleteVolume}
  />

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
