<script lang="ts">
  import { page } from "$app/stores";
  import { 
    ArrowLeft,
    HardDrive, 
    Database, 
    History, 
    Camera,
    Link,
    Trash2,
    Activity,
    Server,
    Zap
  } from "lucide-svelte";
  import {
    Card,
    CardHeader,
    CardTitle,
    CardDescription,
    CardContent,
    Button,
    ButtonGroup,
    Badge,
    EmptyState,
    Separator,
    Field,
    FieldGroup,
    Input,
  } from "$lib/components";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import { formatDate } from "$lib/utils";
  import type { AttachedVolume, Volume, VolumeAttachmentInfo, VolumeWithAttachments } from "$lib/api";
  import { volumesStore } from "$lib/stores/volumes";

  const volumeId = $page.params.volumeId;
  let volume: Volume | AttachedVolume | VolumeWithAttachments | undefined;
  let attachments: VolumeAttachmentInfo[];

  $: volume = $volumesStore.find((v) => v.id === volumeId);
  $: attachments = volume && "attachments" in volume ? (volume.attachments as VolumeAttachmentInfo[]) : [];
  $: attachmentCount = attachments.length;
  $: isAttached = attachmentCount > 0;
  $: volumeStatusLabel = isAttached ? "Attached" : "Available";
  $: volumeUpdatedAt = "updated_at" in (volume || {}) ? (volume as AttachedVolume | VolumeWithAttachments).updated_at : volume?.created_at || "";
  $: volumePoolName = volume && "pool_name" in volume ? volume.pool_name : "ceph-rbd-ssd";

  let activeTab: "overview" | "snapshots" | "settings" = "overview";
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
            <Button variant="outline" size="sm">
              <Camera class="size-4" />
              Take Snapshot
            </Button>
            <Button variant="destructive-soft" size="sm">
              <Trash2 class="size-4" />
              Delete
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

      <ButtonGroup class="w-full flex-wrap">
        <Button
          variant={activeTab === "overview" ? "secondary" : "outline"}
          onclick={() => (activeTab = "overview")}
        >
          Overview
        </Button>
        <Button
          variant={activeTab === "snapshots" ? "secondary" : "outline"}
          onclick={() => (activeTab = "snapshots")}
        >
          Snapshots
        </Button>
        <Button
          variant={activeTab === "settings" ? "secondary" : "outline"}
          onclick={() => (activeTab = "settings")}
        >
          Settings
        </Button>
      </ButtonGroup>

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
                        <Button variant="ghost" size="sm" class="text-destructive hover:bg-destructive/10">
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
                      <Button variant="outline" size="sm" class="mt-3">
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
          <CardHeader>
            <CardTitle class="text-base">Snapshot History</CardTitle>
            <CardDescription>Recover data or clone this volume from a point in time.</CardDescription>
          </CardHeader>
          <CardContent>
            <EmptyState class="py-12">
              <History class="mb-4 size-10 text-muted-foreground/30" />
              <p class="text-sm text-muted-foreground">No snapshots available for this volume.</p>
              <Button variant="outline" class="mt-4">
                <Camera class="mr-2 size-4" />
                Create first snapshot
              </Button>
            </EmptyState>
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
              <Field label="Volume Name" forId="vol-name" description="Update the user-facing name without changing the storage identity.">
                <div class="flex gap-2">
                  <Input id="vol-name" value={volume.name} />
                  <Button size="sm">Update</Button>
                </div>
              </Field>
            </FieldGroup>

            <Separator />

            <Card size="sm" class="border-destructive/20 bg-destructive/5 shadow-none">
              <CardContent class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                <div class="flex flex-col gap-0.5">
                  <span class="text-sm font-semibold text-destructive">Danger Zone</span>
                  <span class="text-xs text-muted-foreground">Irreversible actions for this volume.</span>
                </div>
                <Button variant="destructive" size="sm">Delete Volume</Button>
              </CardContent>
            </Card>
          </CardContent>
        </Card>
      {/if}
    </div>
  {/if}
</DashboardLayout>
