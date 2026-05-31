<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { Boxes, Pencil, Plus, ArrowRight, Trash2 } from "lucide-svelte";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import {
    AlertDialog,
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
    Modal,
    Skeleton,
  } from "$lib/components";
  import {
    createProject,
    deleteProject,
    updateProject,
    type ProjectInfo,
  } from "$lib/api";
  import { getToken } from "$lib/auth";
  import { toast } from "$lib/toast";
  import {
    activeProjectSlugStore,
    activeProjectStore,
    beginProjectSwitch,
    projectsError,
    projectsLoading,
    projectsStore,
    refreshProjects,
    setActiveProjectSlug,
  } from "$lib/stores/projects";
  import { cn, formatDate } from "$lib/utils";
  import { matchesSearch } from "$lib/search";

  let projectName = "";
  let creating = false;
  let renameOpen = false;
  let deleteOpen = false;
  let renameSaving = false;
  let deleteSaving = false;
  let renameTarget: ProjectInfo | null = null;
  let deleteTarget: ProjectInfo | null = null;
  let renameName = "";
  let query = "";
  let sortedProjects: ProjectInfo[];

  $: sortedProjects = [...$projectsStore].sort(
    (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
  );
  $: filteredProjects = sortedProjects.filter((project) =>
    matchesSearch([project.name, project.tenant_id], query)
  );

  onMount(() => {
    if ($projectsStore.length === 0) {
      void refreshProjects();
    }
  });

  function switchProject(slug: string) {
    if (slug === $activeProjectSlugStore) return;
    beginProjectSwitch();
    setActiveProjectSlug(slug);
    void goto("/", {
      replaceState: true,
      invalidateAll: true,
      noScroll: true,
      keepFocus: true,
    });
  }

  function openRename(project: ProjectInfo) {
    renameTarget = project;
    renameName = project.name;
    renameOpen = true;
  }

  function openDelete(project: ProjectInfo) {
    deleteTarget = project;
    deleteOpen = true;
  }

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

  async function handleRenameProject(event: SubmitEvent) {
    event.preventDefault();
    if (!renameTarget) return;

    const token = getToken();
    if (!token) {
      toast.error("You must be logged in to rename a project");
      return;
    }

    const name = renameName.trim();
    if (!name) {
      toast.error("Project name is required");
      return;
    }

    renameSaving = true;
    try {
      const result = await updateProject(token, renameTarget.tenant_id, { name });
      if (result.error || !result.data) {
        toast.error(result.error || "Failed to update project");
        return;
      }

      toast.success(`Project renamed to ${result.data.name}`);
      renameOpen = false;
      renameTarget = null;
      renameName = "";
      await refreshProjects();
    } finally {
      renameSaving = false;
    }
  }

  async function handleDeleteProject() {
    if (!deleteTarget) return;

    const token = getToken();
    if (!token) {
      toast.error("You must be logged in to delete a project");
      return;
    }

    deleteSaving = true;
    try {
      const result = await deleteProject(token, deleteTarget.tenant_id);
      if (result.error) {
        toast.error(result.error);
        return;
      }

      toast.success(`Project ${deleteTarget.name} deleted`);
      deleteOpen = false;
      deleteTarget = null;
      await refreshProjects();
    } finally {
      deleteSaving = false;
    }
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
          Manage the tenant boundary that scopes apps, storage and databases in Mikrom.
        </p>
      </div>
      {#if $activeProjectStore}
        <Badge variant="secondary">Active: {$activeProjectStore.name}</Badge>
      {/if}
    </div>

    <Card size="sm" class="overflow-hidden">
      <CardContent>
        <Input bind:value={query} placeholder="Search by project name or slug" />
      </CardContent>
    </Card>

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
                <Plus data-icon="inline-start" />
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
            Switch, rename or delete projects. Delete is only allowed when the project has no apps, databases or volumes.
          </CardDescription>
        </CardHeader>
        <CardContent class="flex flex-col gap-4">
          {#if $projectsLoading && $projectsStore.length === 0}
            <div class="grid gap-3">
              {#each Array.from({ length: 3 }) as _}
                <Card size="sm" class="overflow-hidden border-border/70 bg-background/60 shadow-none">
                  <CardContent class="flex flex-col gap-3">
                    <Skeleton class="h-4 w-40" />
                    <Skeleton class="h-3 w-28" />
                  </CardContent>
                </Card>
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
          {:else if filteredProjects.length === 0}
            <EmptyState class="py-12">
              <Boxes class="size-10 text-muted-foreground" />
              <h2 class="text-xl font-semibold">{query ? "No matching projects" : "No projects yet"}</h2>
              <p class="max-w-md text-sm text-muted-foreground">
                {query
                  ? "Try a different search term or clear the search box."
                  : "Create your first project to start isolating apps and resources."}
              </p>
            </EmptyState>
          {:else}
            <div class="grid gap-3">
              {#each filteredProjects as project}
                <Card
                  data-project-slug={project.tenant_id}
                  class={cn(
                    "transition-colors",
                    project.tenant_id === $activeProjectStore?.tenant_id
                      ? "border-border/80 bg-muted/35"
                      : "border-border/70 hover:bg-muted/20"
                  )}
                >
                  <CardContent class="flex flex-col gap-4 md:flex-row md:items-start md:justify-between">
                    <div class="min-w-0 flex-1">
                      <div class="flex flex-wrap items-center gap-2">
                        <h3 class="truncate text-base font-semibold">{project.name}</h3>
                        {#if project.tenant_id === $activeProjectStore?.tenant_id}
                          <Badge variant="secondary">Active</Badge>
                        {/if}
                      </div>
                      <p class="mt-1 truncate text-xs text-muted-foreground">Slug: {project.tenant_id}</p>
                      <p class="mt-1 text-xs text-muted-foreground">Created {formatDate(project.created_at)}</p>
                      <p class="mt-1 text-xs text-muted-foreground">Updated {formatDate(project.updated_at || project.created_at)}</p>
                    </div>

                    <div class="flex flex-wrap items-center gap-2">
                      <Button
                        size="sm"
                        variant={project.tenant_id === $activeProjectStore?.tenant_id ? "secondary" : "outline"}
                        disabled={project.tenant_id === $activeProjectStore?.tenant_id}
                        onclick={() => switchProject(project.tenant_id)}
                      >
                        <ArrowRight data-icon="inline-start" />
                        Switch
                      </Button>
                      <Button size="sm" variant="outline" onclick={() => openRename(project)}>
                        <Pencil data-icon="inline-start" />
                        Rename
                      </Button>
                      <Button size="sm" variant="destructive" onclick={() => openDelete(project)}>
                        <Trash2 data-icon="inline-start" />
                        Delete
                      </Button>
                    </div>
                  </CardContent>
                </Card>
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
          <p class="text-sm font-medium">{$activeProjectStore?.name || "No active project"}</p>
          <p class="truncate text-xs text-muted-foreground">
            {$activeProjectStore?.tenant_id || "Select or create a project to continue"}
          </p>
        </div>
        <Button variant="secondary" onclick={() => goto("/")}>
          Go to dashboard
        </Button>
      </CardContent>
    </Card>
  </div>
</DashboardLayout>

<Modal
  bind:open={renameOpen}
  title="Rename project"
  description={renameTarget ? `Update the display name for ${renameTarget.tenant_id}.` : ""}
  width="max-w-md"
  onclose={() => {
    renameTarget = null;
    renameName = "";
  }}
>
  <form class="flex flex-col gap-5" on:submit|preventDefault={handleRenameProject}>
    <FieldGroup>
      <Field label="Project name" forId="rename_project_name">
        <Input id="rename_project_name" bind:value={renameName} autocomplete="off" required />
      </Field>
    </FieldGroup>

    <div class="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
      <Button variant="outline" type="button" onclick={() => (renameOpen = false)}>Cancel</Button>
      <Button type="submit" disabled={renameSaving}>
        {renameSaving ? "Saving..." : "Save changes"}
      </Button>
    </div>
  </form>
</Modal>

<AlertDialog
  bind:open={deleteOpen}
  title="Delete project?"
  description={
    deleteTarget
      ? `This will delete ${deleteTarget.name}. The API will block the operation if the project still has apps, databases or volumes.`
      : ""
  }
  confirmLabel={deleteSaving ? "Deleting..." : "Delete project"}
  confirmVariant="destructive"
  loading={deleteSaving}
  onconfirm={handleDeleteProject}
  onclose={() => {
    deleteTarget = null;
  }}
/>
