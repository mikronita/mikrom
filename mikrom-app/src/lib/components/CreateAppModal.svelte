<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { GitPullRequest, Globe } from "lucide-svelte";
  import { 
    Modal, 
    Button, 
    Input, 
    Select, 
    SelectContent, 
    SelectItem, 
    SelectTrigger, 
    SelectValue, 
    Field, 
    Skeleton 
  } from "$lib/components";
  import { createApp, getGithubInstallUrl, listGithubRepos, type GithubRepo, type CreateAppRequest } from "$lib/api";
  import { getToken } from "$lib/auth";
  import { toast } from "$lib/toast";
  import { activeProjectSlugStore, activeProjectStore } from "$lib/stores/projects";

  let {
    open = $bindable(false),
    onClose = undefined,
  } = $props<{
    open?: boolean;
    onClose?: (() => void) | undefined;
  }>();

  let name = $state("");
  let gitUrl = $state("");
  let activeTab = $state<"manual" | "github">("manual");
  let githubRepos = $state<GithubRepo[]>([]);
  let selectedRepoId = $state("");
  let loadingRepos = $state(false);

  let selectedRepo = $derived(githubRepos.find((repo) => repo.id.toString() === selectedRepoId));

  onMount(async () => {
    const token = getToken();
    if (!token) return;
    loadingRepos = true;
    const result = await listGithubRepos(token);
    if (result.data) githubRepos = result.data;
    loadingRepos = false;
  });

  function close() {
    open = false;
    onClose?.();
  }

  async function handleConnectGithub() {
    const token = getToken();
    if (!token) {
      toast.error("You must be logged in to connect GitHub");
      return;
    }

    const loadingId = toast.loading("Redirecting to GitHub...");
    const result = await getGithubInstallUrl(token);
    toast.dismiss(loadingId);
    if (result.data?.url) {
      window.location.href = result.data.url;
      return;
    }
    toast.error(result.error || "Failed to get installation URL");
  }

  async function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    const token = getToken();
    if (!token) {
      toast.error("You must be logged in");
      return;
    }

    if (!$activeProjectSlugStore) {
      toast.error("Select a project before creating an app");
      return;
    }

    const payload: CreateAppRequest =
      activeTab === "github" && selectedRepo
        ? {
            name,
            git_url: selectedRepo.html_url,
            github_installation_id: selectedRepo.installation_id,
            github_repo_id: selectedRepo.id,
            github_repo_full_name: selectedRepo.full_name,
          }
        : { name, git_url: gitUrl };

    const result = await createApp(token, payload);
    if (result.error) {
      toast.error(result.error);
      return;
    }

    toast.success(`App ${name} created successfully`);
    close();
    if (result.data?.name) {
      goto(`/apps/${encodeURIComponent(result.data.name)}`);
    }
  }

  function selectTab(tab: "manual" | "github") {
    activeTab = tab;
  }
</script>

<Modal bind:open title="Create New Application" description="Add a Git-backed project to the active project." width="max-w-[425px]" onclose={close}>
  <form class="flex flex-col gap-6 pt-2" onsubmit={handleSubmit}>
    <Field label="App Name" forId="app_name">
      <Input id="app_name" bind:value={name} placeholder="my-cool-project" required />
    </Field>

    <Field
      label="Project scope"
      description="This application will be created in the currently active project."
    >
      <div class="rounded-md border border-border bg-muted/30 px-3 py-2">
        <div class="text-sm font-medium">
          {$activeProjectStore?.name || $activeProjectSlugStore || "No active project"}
        </div>
        {#if $activeProjectStore?.tenant_id || $activeProjectSlugStore}
          <div class="text-xs text-muted-foreground">
            {$activeProjectStore?.tenant_id || $activeProjectSlugStore}
          </div>
        {/if}
      </div>
    </Field>

    <div class="grid w-full grid-cols-2 rounded-md border border-border bg-muted p-1">
      <button
        type="button"
        class={`inline-flex items-center justify-center gap-2 rounded px-3 py-2 text-sm transition-colors ${
          activeTab === "manual" ? "bg-background shadow-sm text-foreground" : "text-muted-foreground hover:text-foreground"
        }`}
        onclick={() => selectTab("manual")}
      >
        <Globe class="size-4" />
        Manual URL
      </button>
      <button
        type="button"
        class={`inline-flex items-center justify-center gap-2 rounded px-3 py-2 text-sm transition-colors ${
          activeTab === "github" ? "bg-background shadow-sm text-foreground" : "text-muted-foreground hover:text-foreground"
        }`}
        onclick={() => selectTab("github")}
      >
        <GitPullRequest class="size-4" />
        GitHub
      </button>
    </div>

    {#if activeTab === "manual"}
      <Field label="Git Repository URL" forId="git_url" description="Public repositories only. For private ones, use the GitHub integration.">
        <Input id="git_url" bind:value={gitUrl} placeholder="https://github.com/user/repo" required />
      </Field>
    {:else}
      <Field label="Select Repository" forId="github_repo">
        {#if loadingRepos}
          <div class="flex flex-col gap-2 rounded-md border border-border p-4">
            <Skeleton class="h-4 w-40" />
            <Skeleton class="h-10 w-full" />
          </div>
        {:else if githubRepos.length > 0}
          <Select bind:value={selectedRepoId}>
            <SelectTrigger id="github_repo">
              <SelectValue placeholder="Select a repository" />
            </SelectTrigger>
            <SelectContent>
              {#each githubRepos as repo}
                <SelectItem value={repo.id.toString()}>
                  {repo.full_name}{repo.private ? " (private)" : ""}
                </SelectItem>
              {/each}
            </SelectContent>
          </Select>
        {:else}
          <div class="flex flex-col items-center gap-4 rounded-md border border-border p-6 text-center">
            <p class="text-sm text-muted-foreground">No GitHub accounts connected.</p>
            <Button variant="outline" size="sm" type="button" onclick={handleConnectGithub}>
              Connect GitHub
            </Button>
          </div>
        {/if}
      </Field>
    {/if}

    <div class="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
      <Button variant="outline" type="button" onclick={close}>Cancel</Button>
      <Button type="submit" disabled={!$activeProjectSlugStore || (activeTab === "github" && !selectedRepoId)}>Create App</Button>
    </div>
  </form>
</Modal>
