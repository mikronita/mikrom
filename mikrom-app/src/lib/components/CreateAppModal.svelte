<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { GitPullRequest, Globe, Loader2, Lock } from "lucide-svelte";
  import Modal from "$lib/components/Modal.svelte";
  import Button from "$lib/components/Button.svelte";
  import Input from "$lib/components/Input.svelte";
  import Field from "$lib/components/Field.svelte";
  import { createApp, getGithubInstallUrl, listGithubRepos, type GithubRepo, type CreateAppRequest } from "$lib/api";
  import { getToken } from "$lib/auth";
  import { toast } from "$lib/toast";

  export let open = false;
  export let onClose: (() => void) | undefined = undefined;

  let name = "";
  let gitUrl = "";
  let activeTab: "manual" | "github" = "manual";
  let githubRepos: GithubRepo[] = [];
  let selectedRepoId = "";
  let loadingRepos = false;

  $: selectedRepo = githubRepos.find((repo) => repo.id.toString() === selectedRepoId);

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

<Modal bind:open title="Create New Application" description="Add a Git-backed project to your workspace." width="max-w-[425px]" on:close={close}>
  <form class="flex flex-col gap-6 pt-2" on:submit|preventDefault={handleSubmit}>
    <Field label="App Name" forId="app_name">
      <Input id="app_name" bind:value={name} placeholder="my-cool-project" required />
    </Field>

    <div class="grid w-full grid-cols-2 rounded-md border border-border bg-muted p-1">
      <button
        type="button"
        class={`inline-flex items-center justify-center gap-2 rounded px-3 py-2 text-sm transition-colors ${
          activeTab === "manual" ? "bg-background shadow-sm text-foreground" : "text-muted-foreground hover:text-foreground"
        }`}
        on:click={() => selectTab("manual")}
      >
        <Globe class="size-4" />
        Manual URL
      </button>
      <button
        type="button"
        class={`inline-flex items-center justify-center gap-2 rounded px-3 py-2 text-sm transition-colors ${
          activeTab === "github" ? "bg-background shadow-sm text-foreground" : "text-muted-foreground hover:text-foreground"
        }`}
        on:click={() => selectTab("github")}
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
          <div class="flex items-center justify-center gap-2 rounded-md border border-border p-4 text-sm text-muted-foreground">
            <Loader2 class="size-4 animate-spin" />
            Loading repositories...
          </div>
        {:else if githubRepos.length > 0}
          <div class="relative">
            <select
              id="github_repo"
              bind:value={selectedRepoId}
              class="flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm shadow-none transition-colors placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
            >
              <option value="">Select a repository</option>
              {#each githubRepos as repo}
                <option value={repo.id.toString()}>{repo.full_name}{repo.private ? " (private)" : ""}</option>
              {/each}
            </select>
          </div>
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
      <Button type="submit" disabled={activeTab === "github" && !selectedRepoId}>Create App</Button>
    </div>
  </form>
</Modal>
