<script lang="ts">
  import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle, CardSkeleton } from "$lib/components";
  import { Github, Plus } from "lucide-svelte";
  import type { GithubAccount } from "$lib/api";

  let {
    loadingGithub = false,
    githubAccounts = [],
    onConnectGithub,
  } = $props<{
    loadingGithub?: boolean;
    githubAccounts?: GithubAccount[];
    onConnectGithub: () => Promise<void> | void;
  }>();
</script>

<Card size="sm">
  <CardHeader>
    <CardTitle>Integrations</CardTitle>
    <CardDescription>Connect source control providers and external services.</CardDescription>
  </CardHeader>
  <CardContent>
    <div class="flex flex-col gap-1.5">
      <p class="text-sm font-medium">Source control</p>
      <p class="text-sm text-muted-foreground">Connect your GitHub account to deploy private repositories.</p>

      <div class="mt-4 flex flex-col gap-4">
        {#if loadingGithub}
          <div class="flex flex-col gap-4">
            <CardSkeleton
              compact
              showBadge={false}
              iconClassName="size-10 rounded-md"
              titleClassName="w-32"
              descriptionClassName="w-44"
              footerLineClassName=""
            />
            <CardSkeleton
              compact
              showBadge={false}
              iconClassName="size-10 rounded-md"
              titleClassName="w-32"
              descriptionClassName="w-44"
              footerLineClassName=""
            />
          </div>
        {:else if githubAccounts.length > 0}
          {#each githubAccounts as account}
            <Card size="sm" class="overflow-hidden">
              <CardContent>
                <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                  <div class="flex items-center gap-4">
                    <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
                      <Github class="size-4" />
                    </div>
                    <div class="min-w-0">
                      <p class="truncate text-sm font-semibold">@{account.github_username}</p>
                      <p class="text-xs text-muted-foreground">Connected on {new Date(account.created_at).toLocaleDateString()}</p>
                    </div>
                  </div>
                  <Button variant="outline" size="sm" href={`https://github.com/settings/installations/${account.installation_id}`} target="_blank" rel="noreferrer">
                    Configure
                  </Button>
                </div>
              </CardContent>
            </Card>
          {/each}
          <div>
            <Button variant="outline" size="sm" onclick={onConnectGithub}>
              <Plus class="size-4" />
              Connect another account
            </Button>
          </div>
        {:else}
          <Card size="sm" class="overflow-hidden">
            <CardContent>
              <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                <div class="flex items-center gap-4">
                  <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
                    <Github class="size-4" />
                  </div>
                  <div class="min-w-0">
                    <p class="text-sm font-semibold">GitHub app integration</p>
                    <p class="text-xs text-muted-foreground">Deploy from any repository you have access to.</p>
                  </div>
                </div>
                <Button size="sm" onclick={onConnectGithub}>Connect GitHub</Button>
              </div>
            </CardContent>
          </Card>
        {/if}
      </div>
    </div>
  </CardContent>
</Card>
