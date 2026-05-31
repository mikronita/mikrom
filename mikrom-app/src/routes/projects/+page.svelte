<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { Boxes, Plus, ArrowRight } from "lucide-svelte";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import {
    Badge,
    Button,
    Card,
    CardContent,
    CardDescription,
    CardHeader,
    CardTitle,
    EmptyState,
    Field,
    FieldGroup,
    Input,
    Skeleton,
  } from "$lib/components";
  import { createProject, type ProjectInfo } from "$lib/api";
  import { getToken } from "$lib/auth";
  import { toast } from "$lib/toast";
  import {
    beginProjectSwitch,
    activeProjectStore,
    projectsError,
    projectsLoading,
    projectsStore,
    refreshProjects,
    setActiveProjectSlug,
  } from "$lib/stores/projects";
  import { cn, formatDate } from "$lib/utils";

  let projectName = "";
  let creating = false;
  let sortedProjects: ProjectInfo[] = [];

  $: sortedProjects = [...$projectsStore].sort(
    (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
  );

  onMount(() => {
    if ($projectsStore.length === 0) {
      void refreshProjects();
    }
  });

  async function handleCreateProject(event: SubmitEvent) {
    event.preventDefault();
    const token = getToken();
    if (!token) {
      toast.error("You must be logged in to create a project");
      return;
    }

    const name = projectName.trim();
    if (!name) {
      toast.error("Project name is required");
      return;
    }

    creating = true;
    try {
      const result = await createProject(token, { name });
      if (result.error || !result.data) {
        toast.error(result.error || "Failed to create project");
        return;
      }

      toast.success(`Project ${result.data.name} created`);
      projectName = "";
      beginProjectSwitch();
      setActiveProjectSlug(result.data.tenant_id);
      await goto("/", {
        replaceState: true,
        invalidateAll: true,
        noScroll: true,
        keepFocus: true,
      });
    } finally {
      creating = false;
    }
  }

  function switchProject(slug: string) {
    if (slug === $activeProjectStore?.tenant_id) return;
    beginProjectSwitch();
    setActiveProjectSlug(slug);
    void goto("/", {
      replaceState: true,
      invalidateAll: true,
      noScroll: true,
      keepFocus: true,
    });
  }
</script>

<svelte:head>
  <title>Mikrom - Projects</title>
</svelte:head>

<DashboardLayout>
  <div class="flex flex-col gap-6">
    <div class="flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
      <div class="flex flex-col gap-2">
        <div class="flex items-center gap-3">
          <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
            <Boxes class="size-5" />
          </div>
          <h1 class="text-3xl font-semibold tracking-tight">Projects</h1>
        </div>
        <p class="max-w-2xl text-sm text-muted-foreground">
          Projects are the tenant boundary in Mikrom. Create one here, then switch into it to manage apps, storage and networking.
        </p>
      </div>
      {#if $activeProjectStore}
        <div class="flex items-center gap-2">
          <Badge variant="secondary">Active: {$activeProjectStore.name}</Badge>
        </div>
      {/if}
    </div>

    <div class="grid gap-4 xl:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
      <Card class="overflow-hidden">
        <CardHeader>
          <CardTitle>Create project</CardTitle>
          <CardDescription>
            Start a new tenant boundary. The creator is added as an admin automatically.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form class="flex flex-col gap-5" on:submit|preventDefault={handleCreateProject}>
            <FieldGroup>
              <Field
                label="Project name"
                forId="project_name"
                description="Use a human-readable label. The backend will generate the slug."
              >
                <Input
                  id="project_name"
                  bind:value={projectName}
                  placeholder="Marketing Site"
                  autocomplete="off"
                  required
                />
              </Field>
            </FieldGroup>

            <div class="flex items-center justify-between gap-3">
              <p class="text-xs text-muted-foreground">
                Creating a project keeps you on the current account and switches you into the new project after creation.
              </p>
              <Button type="submit" disabled={creating}>
                <Plus class="size-4" />
                {creating ? "Creating..." : "Create project"}
              </Button>
            </div>
          </form>
        </CardContent>
      </Card>

      <Card class="overflow-hidden">
        <CardHeader>
          <CardTitle>Project list</CardTitle>
          <CardDescription>
            Switch between projects or review their tenant slugs.
          </CardDescription>
        </CardHeader>
        <CardContent class="flex flex-col gap-4">
          {#if $projectsLoading && $projectsStore.length === 0}
            <div class="grid gap-3">
              {#each Array.from({ length: 3 }) as _}
                <div class="rounded-xl border border-border bg-background/60 p-4">
                  <Skeleton class="mb-3 h-4 w-40" />
                  <Skeleton class="h-3 w-28" />
                </div>
              {/each}
            </div>
          {:else if $projectsError}
            <EmptyState class="py-12">
              <Boxes class="size-10 text-muted-foreground" />
              <h2 class="text-xl font-semibold">Unable to load projects</h2>
              <p class="max-w-md text-sm text-muted-foreground">{$projectsError}</p>
              <Button size="sm" variant="secondary" onclick={() => refreshProjects()}>
                Retry
              </Button>
            </EmptyState>
          {:else if sortedProjects.length === 0}
            <EmptyState class="py-12">
              <Boxes class="size-10 text-muted-foreground" />
              <h2 class="text-xl font-semibold">No projects yet</h2>
              <p class="max-w-md text-sm text-muted-foreground">
                Create your first project to start isolating apps and resources.
              </p>
            </EmptyState>
          {:else}
            <div class="grid gap-3">
              {#each sortedProjects as project}
                <div
                  class={cn(
                    "rounded-xl border bg-background/70 p-4 transition-colors",
                    project.tenant_id === $activeProjectStore?.tenant_id
                      ? "border-border/80 bg-muted/35"
                      : "border-border/70 hover:bg-muted/20"
                  )}
                >
                  <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
                    <div class="min-w-0 flex-1">
                      <div class="flex flex-wrap items-center gap-2">
                        <h3 class="truncate text-sm font-semibold">{project.name}</h3>
                        {#if project.tenant_id === $activeProjectStore?.tenant_id}
                          <Badge variant="secondary">Active</Badge>
                        {/if}
                      </div>
                      <p class="mt-1 truncate text-xs text-muted-foreground">Slug: {project.tenant_id}</p>
                      <p class="mt-1 text-xs text-muted-foreground">Created {formatDate(project.created_at)}</p>
                    </div>

                    <div class="flex items-center gap-2">
                      <Button
                        size="sm"
                        variant={project.tenant_id === $activeProjectStore?.tenant_id ? "secondary" : "outline"}
                        disabled={project.tenant_id === $activeProjectStore?.tenant_id}
                        onclick={() => switchProject(project.tenant_id)}
                      >
                        <ArrowRight class="size-4" />
                        Switch
                      </Button>
                    </div>
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </CardContent>
      </Card>
    </div>

    <Card class="overflow-hidden">
      <CardHeader>
        <CardTitle>Current context</CardTitle>
        <CardDescription>
          The active project determines which apps, deployments and volumes you are looking at.
        </CardDescription>
      </CardHeader>
      <CardContent class="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <div class="min-w-0">
          <p class="text-sm font-medium">{ $activeProjectStore?.name || "No active project" }</p>
          <p class="truncate text-xs text-muted-foreground">{ $activeProjectStore?.tenant_id || "Select or create a project to continue" }</p>
        </div>
        <Button variant="secondary" onclick={() => goto("/")}>
          Go to dashboard
        </Button>
      </CardContent>
    </Card>
  </div>
</DashboardLayout>
