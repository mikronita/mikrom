<script lang="ts">
  import { onMount } from "svelte";
  import { Camera, Copy, Database, HardDrive, History, Plus, RotateCcw, Trash2 } from "lucide-svelte";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import Card from "$lib/components/Card.svelte";
  import CardHeader from "$lib/components/CardHeader.svelte";
  import CardTitle from "$lib/components/CardTitle.svelte";
  import CardDescription from "$lib/components/CardDescription.svelte";
  import CardContent from "$lib/components/CardContent.svelte";
  import Badge from "$lib/components/Badge.svelte";
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

  import { getToken } from "$lib/auth";
  import { createVolume, createVolumeSnapshot, cloneVolumeFromSnapshot, deleteVolume, deleteVolumeSnapshot, restoreVolumeSnapshot, type Volume, type VolumeSnapshot } from "$lib/api";
  import { toast } from "$lib/toast";
  import { appsStore, refreshApps } from "$lib/stores/apps";
  import { volumesStore, snapshotsStore, volumesLoading, snapshotsLoading, refreshVolumes, refreshSnapshots } from "$lib/stores/volumes";

  let selectedApp = "";
  let showCreateVolume = false;
  let volumeForSnapshots = "";
  let volumeToDelete: Volume | null = null;
  let snapshotToDelete: VolumeSnapshot | null = null;
  let showSnapshotsModal = false;
  let restoreSnapshotName = "";
  let snapshotToClone = "";
  let cloneName = "";
  let newVolume = { name: "", size_mib: 1024, mount_point: "/data", access_mode: 0 };

  async function loadVolumes(appName: string) {
    const app = $appsStore.find((item) => item.name === appName);
    if (!app) return;
    await refreshVolumes(app.id);
  }

  async function loadSnapshots(volumeId: string) {
    await refreshSnapshots(volumeId);
  }

  onMount(async () => {
    if ($appsStore.length === 0) {
      await refreshApps();
    }
    selectedApp = $appsStore[0]?.name || "";
    if (selectedApp) await loadVolumes(selectedApp);
  });

  async function createNewVolume() {
    const token = getToken();
    const app = $appsStore.find((item) => item.name === selectedApp);
    if (!token || !app) return;
    const result = await createVolume(token, app.id, newVolume);
    if (result.error) return toast.error(result.error);
    toast.success("Volume created successfully");
    newVolume = { name: "", size_mib: 1024, mount_point: "/data", access_mode: 0 };
    showCreateVolume = false;
    // SSE will trigger refreshVolumes
  }

  async function createSnapshot(volumeId: string) {
    const token = getToken();
    if (!token) return;
    const snapName = `snap-${new Date().toISOString().replace(/[:.]/g, "-")}`;
    const result = await createVolumeSnapshot(token, volumeId, { name: snapName });
    if (result.error) return toast.error(result.error);
    toast.success("Snapshot created");
    // SSE will trigger refreshSnapshots (if implemented in workspace listener)
    await loadSnapshots(volumeId);
  }

  async function deleteVolumeNow(id: string) {
    const token = getToken();
    if (!token) return;
    const result = await deleteVolume(token, id);
    if (result.error) return toast.error(result.error);
    toast.success("Volume deleted");
    // SSE will trigger refreshVolumes
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
    await loadVolumes(selectedApp);
  }

  function formatSize(size: number) {
    return size >= 1024 ? `${(size / 1024).toFixed(1)} GiB` : `${size} MiB`;
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
              <option value="">Select application</option>
              {#each $appsStore as app}
                <option value={app.name}>{app.name}</option>
              {/each}
            </Select>
            {#if selectedApp}
              <Button size="sm" onclick={() => (showCreateVolume = true)}>
                <Plus class="size-4" />
                Create Volume
              </Button>
            {/if}
          </div>
        </div>
      </CardHeader>

      <CardContent class="p-0">
        {#if !selectedApp}
          <EmptyState><HardDrive class="size-10 text-muted-foreground" /><h3 class="text-xl font-semibold">Select an application</h3><p class="text-sm text-muted-foreground">Choose an app to manage its persistent storage volumes.</p></EmptyState>
        {:else if $volumesLoading}
          <div class="flex flex-col gap-3 p-4">
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
          </div>
        {:else if $volumesStore.length === 0}
          <EmptyState><HardDrive class="size-10 text-muted-foreground" /><h3 class="text-xl font-semibold">No volumes found</h3><p class="text-sm text-muted-foreground">This application doesn't have any persistent volumes yet.</p><Button size="sm" onclick={() => (showCreateVolume = true)}><Plus class="size-4" />Create first volume</Button></EmptyState>
        {:else}
          <table class="w-full">
            <thead>
              <tr class="border-b border-border text-left text-sm">
                <th class="px-4 py-3">Name</th>
                <th class="px-4 py-3">Size</th>
                <th class="px-4 py-3">Mount Point</th>
                <th class="px-4 py-3">Mode</th>
                <th class="px-4 py-3">Pool</th>
                <th class="px-4 py-3 text-right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {#each $volumesStore as volume}
                <tr class="border-b border-border text-sm">
                  <td class="px-4 py-4 font-medium"><div class="flex items-center gap-2"><Database class="size-4 text-muted-foreground" />{volume.name}</div></td>
                  <td class="px-4 py-4"><Badge variant="secondary">{formatSize(volume.size_mib)}</Badge></td>
                  <td class="px-4 py-4"><code class="text-xs">{volume.mount_point}</code></td>
                  <td class="px-4 py-4">
                    <Badge
                      variant={volume.access_mode === 1 ? "outline" : volume.access_mode === 2 ? "outline" : "secondary"}
                      className={volume.access_mode === 1 ? "border-transparent bg-[color-mix(in_srgb,var(--status-info)_12%,transparent)] text-[var(--status-info)]" : ""}
                    >
                      {volume.access_mode === 1 ? "RWX (Mesh)" : volume.access_mode === 2 ? "ROX (Shared)" : "RWO (Single)"}
                    </Badge>
                  </td>
                  <td class="px-4 py-4 font-mono text-[10px] text-muted-foreground">{volume.pool_name}</td>
                  <td class="px-4 py-4 text-right">
                    <div class="flex justify-end gap-2">
                      <Button variant="ghost" size="icon" onclick={async () => { volumeForSnapshots = volume.id; showSnapshotsModal = true; await loadSnapshots(volume.id); }}>
                        <History class="size-4" />
                      </Button>
                      <Button variant="ghost" size="icon" onclick={() => createSnapshot(volume.id)}>
                        <Camera class="size-4" />
                      </Button>
                      <Button variant="ghost" size="icon" onclick={() => (volumeToDelete = volume)}>
                        <Trash2 class="size-4 text-destructive" />
                      </Button>
                    </div>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </CardContent>
    </Card>
  </div>

  {#if showCreateVolume}
    <Modal bind:open={showCreateVolume} title="Create new volume" description={`The volume will be created for ${selectedApp}.`}>
      <FieldGroup className="pt-2">
        <Field label="Volume name"><Input bind:value={newVolume.name} placeholder="my-data-volume" /></Field>
        <Field label="Size (MiB)"><Input type="number" bind:value={newVolume.size_mib} min={128} /></Field>
        <Field label="Mount point"><Input bind:value={newVolume.mount_point} placeholder="/data" /></Field>
        <Field label="Access Mode">
          <Select bind:value={newVolume.access_mode}>
            <option value={0}>RWO - ReadWriteOnce (Single Node)</option>
            <option value={1}>RWX - ReadWriteMany (Shared Mesh)</option>
            <option value={2}>ROX - ReadOnlyMany (Shared Read)</option>
          </Select>
        </Field>
        <p class="text-xs text-muted-foreground italic">
          Note: This volume will be attached and mounted at this path during the <strong>next deployment</strong> of the application. 
          {#if newVolume.access_mode === 1}
            <span class="text-status-online font-semibold">RWX mode uses CephFS for multi-node sharing.</span>
          {/if}
        </p>
        <div class="flex justify-end gap-2">
          <Button variant="outline" onclick={() => (showCreateVolume = false)}>Cancel</Button>
          <Button onclick={createNewVolume} disabled={!newVolume.name || !newVolume.mount_point}>Create</Button>
        </div>
      </FieldGroup>
    </Modal>
  {/if}

  <AlertDialog
    open={Boolean(volumeToDelete)}
    title="Delete volume?"
    description={`This will permanently delete ${volumeToDelete?.name || "this volume"} and cannot be undone.`}
    confirmLabel="Delete Volume"
    on:close={() => (volumeToDelete = null)}
    on:confirm={confirmDeleteVolume}
  />

  {#if showSnapshotsModal}
    <Modal open={showSnapshotsModal} title="Snapshot history" width="max-w-3xl" description={`Manage snapshots for volume ${$volumesStore.find((v) => v.id === volumeForSnapshots)?.name || ""}.`} on:close={() => { showSnapshotsModal = false; volumeForSnapshots = ""; snapshotsStore.set([]); }}>
      <div class="space-y-4">
        {#if $snapshotsLoading}
          <table class="w-full">
            <TableSkeleton
              rows={3}
              rowClassName="border-b border-border"
              cellClassName="px-4 py-3"
              columns={[
                { skeletonClassName: "h-5 w-40" },
                { skeletonClassName: "h-5 w-32" },
                { skeletonClassName: "ml-auto h-8 w-28", cellClassName: "text-right" },
              ]}
            />
          </table>
        {:else if $snapshotsStore.length === 0}
          <p class="py-8 text-center text-sm text-muted-foreground">No snapshots found for this volume.</p>
        {:else}
          <div class="overflow-x-auto">
            <table class="w-full">
              <thead><tr class="border-b border-border text-left text-sm"><th class="px-4 py-3">Name</th><th class="px-4 py-3">Created At</th><th class="px-4 py-3 text-right">Actions</th></tr></thead>
              <tbody>
                {#each $snapshotsStore as snap}
                  <tr class="border-b border-border">
                    <td class="px-4 py-3 font-medium">{snap.name}</td>
                    <td class="px-4 py-3 text-sm text-muted-foreground">{new Date(snap.created_at).toLocaleString()}</td>
                    <td class="px-4 py-3 text-right">
                      <div class="flex justify-end gap-2">
                        <Button variant="outline" size="sm" onclick={() => { restoreSnapshotName = snap.name; }}>
                          <RotateCcw class="size-3" />
                          Restore
                        </Button>
                        <Button variant="outline" size="sm" onclick={() => { snapshotToClone = snap.volume_id; restoreSnapshotName = snap.name; cloneName = `${$volumesStore.find((v) => v.id === snap.volume_id)?.name || "volume"}-clone`; }}>
                          <Copy class="size-3" />
                          Clone
                        </Button>
                        <Button variant="ghost" size="icon" onclick={() => (snapshotToDelete = snap)}>
                          <Trash2 class="size-4 text-destructive" />
                        </Button>
                      </div>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}

        {#if restoreSnapshotName}
          <div class="space-y-4 rounded-md border border-border bg-muted/20 p-4">
            <Field label="Restore snapshot name"><Input bind:value={restoreSnapshotName} /></Field>
            <div class="flex justify-end gap-2">
              <Button variant="outline" onclick={() => (restoreSnapshotName = "")}>Cancel</Button>
              <Button onclick={restoreSnapshot}>Restore</Button>
            </div>
          </div>
        {/if}

        {#if snapshotToClone}
          <div class="space-y-4 rounded-md border border-border bg-muted/20 p-4">
            <Field label="Clone name"><Input bind:value={cloneName} /></Field>
            <div class="flex justify-end gap-2">
              <Button variant="outline" onclick={() => (snapshotToClone = "")}>Cancel</Button>
              <Button onclick={cloneSnapshot}>Clone</Button>
            </div>
          </div>
        {/if}
      </div>
    </Modal>
  {/if}

  <AlertDialog
    open={Boolean(snapshotToDelete)}
    title="Delete snapshot?"
    description={`This will permanently delete ${snapshotToDelete?.name || "this snapshot"} from Ceph and cannot be undone.`}
    confirmLabel="Delete Snapshot"
    on:close={() => (snapshotToDelete = null)}
    on:confirm={confirmDeleteSnapshot}
  />
</DashboardLayout>
