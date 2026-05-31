<script lang="ts">
  import { page } from "$app/stores";
  import { 
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
    EmptyState,
  } from "$lib/components";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import type { AttachedVolume, Volume, VolumeAttachmentInfo, VolumeWithAttachments } from "$lib/api";
  import { volumesStore } from "$lib/stores/volumes";

  const volumeId = $page.params.volumeId;
  let volume: Volume | AttachedVolume | VolumeWithAttachments | undefined;
  let attachments: VolumeAttachmentInfo[] = [];

  $: volume = $volumesStore.find((v) => v.id === volumeId);
  $: attachments = volume && "attachments" in volume ? (volume.attachments as VolumeAttachmentInfo[]) : [];

  let activeTab: "overview" | "snapshots" | "settings" = "overview";
</script>

<svelte:head>
  <title>Mikrom - {volume?.name || "Volume"}</title>
</svelte:head>

<DashboardLayout>
  {#if !volume}
    <div class="flex flex-col items-center justify-center py-20">
      <EmptyState>
        <HardDrive class="size-10 text-muted-foreground" />
        <h2 class="text-xl font-semibold">Volume not found</h2>
        <p class="text-sm text-muted-foreground">The volume you are looking for does not exist or has been deleted.</p>
        <Button href="/storage" variant="outline" class="mt-4">
          Back to Storage
        </Button>
      </EmptyState>
    </div>
  {:else}
    <div class="flex flex-col gap-6">
      <div class="flex flex-col gap-4">
        <div class="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
          <div class="flex items-center gap-4">
            <div class="flex size-12 items-center justify-center rounded-lg border border-border bg-background text-foreground">
              <HardDrive class="size-6" />
            </div>
            <div class="flex flex-col">
              <h1 class="text-3xl font-bold tracking-tight">{volume.name}</h1>
              <div class="flex items-center gap-2 text-sm text-muted-foreground">
                <span class="font-mono">{volume.id}</span>
                <span>•</span>
                <span>{volume.size_mib} MiB</span>
              </div>
            </div>
          </div>
          <div class="flex items-center gap-2">
            <Button variant="outline" size="sm">
              <Camera class="mr-2 size-4" />
              Take Snapshot
            </Button>
            <Button variant="destructive-soft" size="sm">
              <Trash2 class="mr-2 size-4" />
              Delete
            </Button>
          </div>
        </div>
      </div>

      <div class="flex border-b border-border">
        <button 
          class={`px-4 py-2 text-sm font-medium transition-colors hover:text-foreground ${activeTab === "overview" ? "border-b-2 border-primary text-foreground" : "text-muted-foreground"}`}
          onclick={() => (activeTab = "overview")}
        >
          Overview
        </button>
        <button 
          class={`px-4 py-2 text-sm font-medium transition-colors hover:text-foreground ${activeTab === "snapshots" ? "border-b-2 border-primary text-foreground" : "text-muted-foreground"}`}
          onclick={() => (activeTab = "snapshots")}
        >
          Snapshots
        </button>
        <button 
          class={`px-4 py-2 text-sm font-medium transition-colors hover:text-foreground ${activeTab === "settings" ? "border-b-2 border-primary text-foreground" : "text-muted-foreground"}`}
          onclick={() => (activeTab = "settings")}
        >
          Settings
        </button>
      </div>

      {#if activeTab === "overview"}
        <div class="grid gap-6 md:grid-cols-3">
          <Card class="md:col-span-2">
            <CardHeader>
              <CardTitle>Volume Details</CardTitle>
              <CardDescription>General information about this persistent volume.</CardDescription>
            </CardHeader>
            <CardContent class="grid gap-6">
              <div class="grid gap-4 sm:grid-cols-2">
                <div class="flex flex-col gap-1">
                  <span class="text-xs font-medium text-muted-foreground">Status</span>
                  <div class="flex items-center gap-2">
                    <div class="size-2 rounded-full bg-status-online"></div>
                    <span class="text-sm font-medium">Ready</span>
                  </div>
                </div>
                <div class="flex flex-col gap-1">
                  <span class="text-xs font-medium text-muted-foreground">Size</span>
                  <span class="text-sm font-medium">{volume.size_mib} MiB</span>
                </div>
                <div class="flex flex-col gap-1">
                  <span class="text-xs font-medium text-muted-foreground">Type</span>
                  <span class="text-sm font-medium text-foreground flex items-center gap-1.5">
                    <Zap class="size-3 text-amber-500" />
                    Ceph RBD (NVMe)
                  </span>
                </div>
                <div class="flex flex-col gap-1">
                  <span class="text-xs font-medium text-muted-foreground">Replication</span>
                  <span class="text-sm font-medium">3x Replicated</span>
                </div>
              </div>
              
              <div class="border-t border-border pt-6">
                <h3 class="mb-4 text-sm font-semibold">Current Attachments</h3>
                <div class="grid gap-3">
                  {#if attachments.length > 0}
                    {#each attachments as attachment}
                      <div class="flex items-center justify-between rounded-md border border-border p-3">
                        <div class="flex items-center gap-3">
                          <div class="flex size-8 items-center justify-center rounded bg-muted">
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
                    <div class="flex flex-col items-center justify-center rounded-md border border-dashed border-border py-8">
                      <Link class="mb-2 size-6 text-muted-foreground/50" />
                      <p class="text-xs text-muted-foreground">This volume is not attached to any application.</p>
                      <Button variant="outline" size="sm" class="mt-3">
                        Attach to Application
                      </Button>
                    </div>
                  {/if}
                </div>
              </div>
            </CardContent>
          </Card>

          <div class="flex flex-col gap-6">
            <Card>
              <CardHeader>
                <CardTitle class="text-sm">Storage Pool</CardTitle>
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

            <Card>
              <CardHeader>
                <CardTitle class="text-sm">Usage Stats</CardTitle>
              </CardHeader>
              <CardContent class="grid gap-4">
                <div class="space-y-2">
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
        <Card>
          <CardHeader>
            <CardTitle>Snapshot History</CardTitle>
            <CardDescription>Recover data or clone this volume from a point in time.</CardDescription>
          </CardHeader>
          <CardContent>
            <div class="flex flex-col items-center justify-center py-12">
              <History class="mb-4 size-10 text-muted-foreground/30" />
              <p class="text-sm text-muted-foreground">No snapshots available for this volume.</p>
              <Button variant="outline" class="mt-4">
                <Camera class="mr-2 size-4" />
                Create first snapshot
              </Button>
            </div>
          </CardContent>
        </Card>
      {:else if activeTab === "settings"}
        <Card>
          <CardHeader>
            <CardTitle>Volume Settings</CardTitle>
            <CardDescription>Configure volume parameters and management.</CardDescription>
          </CardHeader>
          <CardContent class="grid gap-6">
            <div class="grid gap-2">
              <label for="vol-name" class="text-sm font-medium">Volume Name</label>
              <div class="flex gap-2">
                <input id="vol-name" value={volume.name} class="flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring" />
                <Button size="sm">Update</Button>
              </div>
            </div>
            
            <div class="border-t border-border pt-6">
              <h3 class="text-sm font-semibold text-destructive">Danger Zone</h3>
              <p class="mb-4 text-xs text-muted-foreground">Irreversible actions for this volume.</p>
              <div class="flex flex-col gap-3 rounded-md border border-destructive/20 bg-destructive/5 p-4">
                <div class="flex items-center justify-between">
                  <div class="flex flex-col gap-0.5">
                    <span class="text-sm font-semibold">Delete Volume</span>
                    <span class="text-xs text-muted-foreground">This will permanently delete all data.</span>
                  </div>
                  <Button variant="destructive" size="sm">Delete Volume</Button>
                </div>
              </div>
            </div>
          </CardContent>
        </Card>
      {/if}
    </div>
  {/if}
</DashboardLayout>
