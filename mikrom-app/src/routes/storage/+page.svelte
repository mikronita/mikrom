<script lang="ts">
  import { onMount } from "svelte";
  import { Camera, Copy, Database, HardDrive, History, Link, Plus, RotateCcw, Trash2 } from "lucide-svelte";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import Card from "$lib/components/Card.svelte";
  import CardHeader from "$lib/components/CardHeader.svelte";
  import CardTitle from "$lib/components/CardTitle.svelte";
  import CardDescription from "$lib/components/CardDescription.svelte";
  import CardContent from "$lib/components/CardContent.svelte";
  import Button from "$lib/components/Button.svelte";
  import AlertDialog from "$lib/components/AlertDialog.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";
  import Skeleton from "$lib/components/Skeleton.svelte";
  import TableSkeleton from "$lib/components/TableSkeleton.svelte";
  import Modal from "$lib/components/Modal.svelte";
  import Field from "$lib/components/Field.svelte";
  import FieldGroup from "$lib/components/FieldGroup.svelte";
  import Input from "$lib/components/Input.svelte";
  import Select from "$lib/components/Select.svelte";
  import { Badge as BadgeUI } from "$lib/components/ui/badge";
  import { Button as ButtonUI } from "$lib/components/ui/button";
  import * as TableUI from "$lib/components/ui/table";

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
            <Select bind:value={selectedApp} class="sm:w-[220px]" on:change={async () => await loadVolumes(selectedApp)}>
              <option value="">All Applications</option>
              {#each $appsStore as app}
                <option value={app.name}>{app.name}</option>
              {/each}
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
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
          </div>
        {:else if $volumesStore.length === 0}
          <EmptyState><HardDrive class="size-10 text-muted-foreground" /><h3 class="text-xl font-semibold">No volumes found</h3><p class="text-sm text-muted-foreground">You don't have any persistent volumes yet.</p></EmptyState>
        {:else}
          <TableUI.Root>
            <TableUI.Header>
              <TableUI.Row>
                <TableUI.Head>Volume</TableUI.Head>
                <TableUI.Head>Attached To</TableUI.Head>
                <TableUI.Head class="text-center w-[120px]">Size</TableUI.Head>
                <TableUI.Head class="text-right w-[180px]">Actions</TableUI.Head>
              </TableUI.Row>
            </TableUI.Header>
            <TableUI.Body>
              {#each $volumesStore as volume (volume.id)}
                <TableUI.Row class="group">
                  <TableUI.Cell class="font-medium">
                    <div class="flex items-center gap-3">
                      <div class="flex size-8 shrink-0 items-center justify-center rounded-md border border-border bg-muted/30 text-muted-foreground group-hover:bg-background transition-colors">
                        <Database class="size-4" />
                      </div>
                      <div class="flex flex-col min-w-0">
                        <span class="truncate font-semibold text-sm">{volume.name}</span>
                        <span class="font-mono text-[10px] text-muted-foreground/70 lowercase tracking-wider">{volume.id}</span>
                      </div>
                    </div>
                  </TableUI.Cell>
                  
                  <TableUI.Cell>
                    <div class="flex flex-wrap gap-2">
                      {#if isAttachedVolume(volume)}
                        <BadgeUI variant="outline" class="h-7 items-center gap-1.5 px-2 group/badge bg-muted/20 border-border/50">
                          <span class="font-medium text-foreground">{$appsStore.find((a) => a.id === selectedAppId)?.name || selectedApp || "Current App"}</span>
                          <BadgeUI variant="secondary" class="h-4 px-1 text-[8px] font-bold uppercase tracking-tighter">
                            {volume.access_mode === 1 ? "RWX" : volume.access_mode === 2 ? "ROX" : "RWO"}
                          </BadgeUI>
                          <code class="text-[9px] text-muted-foreground bg-background/50 px-1 rounded border border-border/30">{volume.mount_point}</code>
                          <button 
                            class="ml-0.5 rounded-full p-0.5 hover:bg-destructive/10 hover:text-destructive transition-colors text-muted-foreground" 
                            title="Detach"
                            onclick={() => detachVolumeNow(selectedAppId, volume.id)}
                          >
                            <Plus class="size-3 rotate-45" />
                          </button>
                        </BadgeUI>
                      {:else if isVolumeWithAttachments(volume)}
                        {#if volume.attachments.length === 0}
                          <span class="text-xs text-muted-foreground italic opacity-50">Not attached</span>
                        {:else}
                          {#each volume.attachments as attachment}
                            <BadgeUI variant="outline" class="h-7 items-center gap-1.5 px-2 group/badge bg-muted/20 border-border/50">
                              <span class="font-medium text-foreground">{attachment.app_name}</span>
                              <BadgeUI variant="secondary" class="h-4 px-1 text-[8px] font-bold uppercase tracking-tighter">
                                {attachment.access_mode === 1 ? "RWX" : attachment.access_mode === 2 ? "ROX" : "RWO"}
                              </BadgeUI>
                              <code class="text-[9px] text-muted-foreground bg-background/50 px-1 rounded border border-border/30">{attachment.mount_point}</code>
                              <button 
                                class="ml-0.5 rounded-full p-0.5 hover:bg-destructive/10 hover:text-destructive transition-colors text-muted-foreground" 
                                title={`Detach from ${attachment.app_name}`}
                                onclick={() => detachVolumeNow(attachment.app_id, volume.id)}
                              >
                                <Plus class="size-3 rotate-45" />
                              </button>
                            </BadgeUI>
                          {/each}
                        {/if}
                      {/if}
                    </div>
                  </TableUI.Cell>

                  <TableUI.Cell class="text-center">
                    <BadgeUI variant="secondary" class="font-mono h-6">{formatSize(volume.size_mib)}</BadgeUI>
                  </TableUI.Cell>
                  
                  <TableUI.Cell class="text-right">
                    <div class="flex justify-end gap-1">
                      {#if !selectedApp}
                        <ButtonUI variant="ghost" size="icon" class="size-8" title="Attach to App" onclick={() => { attachParams.volume_id = volume.id; showAttachVolume = true; }}>
                          <Link class="size-4" />
                        </ButtonUI>
                      {/if}
                      <ButtonUI variant="ghost" size="icon" class="size-8" title="Snapshots" onclick={async () => { volumeForSnapshots = volume.id; showSnapshotsModal = true; await loadSnapshots(volume.id); }}>
                        <History class="size-4" />
                      </ButtonUI>
                      <ButtonUI variant="ghost" size="icon" class="size-8" title="Take Snapshot" onclick={() => createSnapshot(volume.id)}>
                        <Camera class="size-4" />
                      </ButtonUI>
                      <ButtonUI variant="ghost" size="icon" class="size-8 text-destructive hover:text-destructive hover:bg-destructive/10" title="Delete Volume" onclick={() => (volumeToDelete = volume)}>
                        <Trash2 class="size-4" />
                      </ButtonUI>
                    </div>
                  </TableUI.Cell>
                </TableUI.Row>
              {/each}
            </TableUI.Body>
          </TableUI.Root>
        {/if}
      </CardContent>
    </Card>
  </div>

  {#if showCreateVolume}
    <Modal bind:open={showCreateVolume} title="Create new volume" description="The volume will be available to be attached to any application.">
      <FieldGroup className="pt-2">
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
      <FieldGroup className="pt-2">
        <Field label="Select Application">
          <Select bind:value={attachParams.app_id}>
            <option value="">Choose an app...</option>
            {#each $appsStore as app}
              <option value={app.id}>{app.name}</option>
            {/each}
          </Select>
        </Field>
        <Field label="Mount point"><Input bind:value={attachParams.mount_point} placeholder="/data" /></Field>
        <Field label="Access Mode">
          <Select bind:value={attachParams.access_mode}>
            <option value={0}>RWO - ReadWriteOnce (Single Node)</option>
            <option value={1}>RWX - ReadWriteMany (Shared Mesh)</option>
            <option value={2}>ROX - ReadOnlyMany (Shared Read)</option>
          </Select>
        </Field>
      </FieldGroup>
      <div class="mt-6 flex justify-end gap-3">
        <Button variant="outline" onclick={() => (showAttachVolume = false)}>Cancel</Button>
        <Button onclick={attachVolumeNow}>Attach Volume</Button>
      </div>
    </Modal>
  {/if}

  <Modal open={showSnapshotsModal} title="Snapshot history" width="max-w-3xl" description={`Manage snapshots for volume ${$volumesStore.find((v) => v.id === volumeForSnapshots)?.name || ""}.`} on:close={() => { showSnapshotsModal = false; volumeForSnapshots = ""; snapshotsStore.set([]); }}>
    <div class="mt-4">
      {#if $snapshotsLoading}
        <TableSkeleton rows={3} cols={3} />
      {:else if $snapshotsStore.length === 0}
        <EmptyState className="py-8"><Camera class="size-8 text-muted-foreground" /><p class="text-sm text-muted-foreground">No snapshots yet.</p></EmptyState>
      {:else}
        <TableUI.Root>
          <TableUI.Header>
            <TableUI.Row>
              <TableUI.Head>Snapshot Name</TableUI.Head>
              <TableUI.Head>Created</TableUI.Head>
              <TableUI.Head class="text-right">Actions</TableUI.Head>
            </TableUI.Row>
          </TableUI.Header>
          <TableUI.Body>
            {#each $snapshotsStore as snap}
              <TableUI.Row>
                <TableUI.Cell class="font-mono text-xs">{snap.name}</TableUI.Cell>
                <TableUI.Cell class="text-muted-foreground text-xs">{new Date(snap.created_at).toLocaleString()}</TableUI.Cell>
                <TableUI.Cell class="text-right">
                  <div class="flex justify-end gap-1">
                    <ButtonUI variant="outline" size="icon" class="size-7" title="Restore" onclick={() => { restoreSnapshotName = snap.name; restoreSnapshot(); }}>
                      <RotateCcw class="size-3.5" />
                    </ButtonUI>
                    <ButtonUI variant="outline" size="icon" class="size-7" title="Clone" onclick={() => { snapshotToClone = snap.volume_id; restoreSnapshotName = snap.name; cloneName = `${$volumesStore.find((v) => v.id === snap.volume_id)?.name || "volume"}-clone`; showCloneModal = true; }}>
                      <Copy class="size-3.5" />
                    </ButtonUI>
                    <ButtonUI variant="ghost" size="icon" class="size-7 text-destructive hover:bg-destructive/10" title="Delete" onclick={() => (snapshotToDelete = snap)}>
                      <Trash2 class="size-3.5" />
                    </ButtonUI>
                  </div>
                </TableUI.Cell>
              </TableUI.Row>
            {/each}
          </TableUI.Body>
        </TableUI.Root>
      {/if}
    </div>
  </Modal>

  <Modal
    bind:open={showCloneModal}
    title="Clone snapshot"
    description={`Create a new volume from snapshot ${restoreSnapshotName}.`}
    on:close={closeCloneModal}
  >
    <FieldGroup className="pt-2">
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
    confirmLabel="Delete Volume"
    confirmVariant="destructive"
    on:close={() => (volumeToDelete = null)}
    on:confirm={confirmDeleteVolume}
  />

  <AlertDialog
    open={!!snapshotToDelete}
    title="Delete snapshot?"
    description={`Are you sure you want to delete snapshot ${snapshotToDelete?.name}?`}
    confirmLabel="Delete Snapshot"
    confirmVariant="destructive"
    on:close={() => (snapshotToDelete = null)}
    on:confirm={confirmDeleteSnapshot}
  />
</DashboardLayout>
