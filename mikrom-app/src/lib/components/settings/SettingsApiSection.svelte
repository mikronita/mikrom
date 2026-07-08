<script lang="ts">
  import { onMount } from "svelte";
  import { getToken } from "$lib/auth";
  import {
    listPersonalAccessTokens,
    createPersonalAccessToken,
    revokePersonalAccessToken,
    type PersonalAccessToken,
  } from "$lib/api";
  import { toast } from "$lib/toast";
  import {
    Button,
    Card,
    CardContent,
    CardDescription,
    CardHeader,
    CardTitle,
    Modal,
    Input,
    Field,
    FieldGroup,
    EmptyState,
    TableSkeleton,
  } from "$lib/components";
  import Plus from "@lucide/svelte/icons/plus";
  import Key from "@lucide/svelte/icons/key";
  import Copy from "@lucide/svelte/icons/copy";
  import Check from "@lucide/svelte/icons/check";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Loader2 from "@lucide/svelte/icons/loader-2";

  let tokens = $state<PersonalAccessToken[]>([]);
  let loading = $state(true);
  let modalOpen = $state(false);
  let tokenName = $state("");
  let creating = $state(false);
  let createdRawToken = $state<string | null>(null);
  let copied = $state(false);
  let revokingId = $state<string | null>(null);

  async function loadTokens() {
    const token = getToken();
    if (!token) return;

    loading = true;
    const result = await listPersonalAccessTokens(token);
    loading = false;

    if (result.error) {
      toast.error(result.error);
      return;
    }

    if (result.data) {
      tokens = result.data;
    }
  }

  onMount(() => {
    void loadTokens();
  });

  async function handleCreateToken(event: SubmitEvent) {
    event.preventDefault();
    if (!tokenName.trim() || creating) return;

    const token = getToken();
    if (!token) return;

    creating = true;
    const result = await createPersonalAccessToken(token, tokenName.trim());
    creating = false;

    if (result.error) {
      toast.error(result.error);
      return;
    }

    if (result.data) {
      createdRawToken = result.data.token;
      toast.success("Personal access token created");
      tokenName = "";
      void loadTokens();
    }
  }

  async function handleRevokeToken(tokenId: string) {
    const token = getToken();
    if (!token) return;

    revokingId = tokenId;
    const result = await revokePersonalAccessToken(token, tokenId);
    revokingId = null;

    if (result.error) {
      toast.error(result.error);
      return;
    }

    toast.success("Token revoked successfully");
    void loadTokens();
  }

  function handleCloseModal() {
    modalOpen = false;
    createdRawToken = null;
    tokenName = "";
  }

  async function copyToClipboard(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      setTimeout(() => {
        copied = false;
      }, 2000);
    } catch {
      toast.error("Failed to copy token to clipboard");
    }
  }

  function formatDate(dateStr: string) {
    return new Date(dateStr).toLocaleDateString(undefined, {
      year: "numeric",
      month: "long",
      day: "numeric",
    });
  }
</script>

<Card size="sm">
  <CardHeader class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
    <div class="flex flex-col gap-1.5">
      <CardTitle>Personal access tokens</CardTitle>
      <CardDescription>Use tokens to authenticate with the Mikrom CLI and API.</CardDescription>
    </div>
    <Button size="sm" onclick={() => (modalOpen = true)}>
      <Plus class="size-4" />
      Create token
    </Button>
  </CardHeader>
  <CardContent>
    {#if loading}
      <TableSkeleton rows={2} cols={1} />
    {:else if tokens.length === 0}
      <EmptyState class="py-12">
        <Key class="size-10 text-muted-foreground" />
        <h2 class="text-xl font-semibold">No active tokens</h2>
        <p class="max-w-md text-sm text-muted-foreground">
          You haven't created any personal access tokens yet. Create one to authenticate with the CLI.
        </p>
        <Button size="sm" onclick={() => (modalOpen = true)}>
          <Plus class="size-4 mr-2" />
          Create first token
        </Button>
      </EmptyState>
    {:else}
      <div class="flex flex-col gap-4">
        {#each tokens as pat (pat.id)}
          <Card size="sm" class="overflow-hidden">
            <CardContent>
              <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                <div class="flex items-center gap-4">
                  <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
                    <Key class="size-4" />
                  </div>
                  <div class="min-w-0">
                    <p class="truncate font-mono text-sm font-semibold">{pat.name}</p>
                    <p class="text-xs text-muted-foreground">
                      Token: <span class="font-mono">mikrom_pat_****************{pat.token_last_four}</span>
                    </p>
                    <p class="text-xs text-muted-foreground">
                      {#if pat.last_used_at}
                        Last used {formatDate(pat.last_used_at)}
                      {:else}
                        Never used
                      {/if}
                      - Created {formatDate(pat.created_at)}
                    </p>
                  </div>
                </div>
                <Button
                  variant="destructive"
                  size="sm"
                  disabled={revokingId === pat.id}
                  onclick={() => void handleRevokeToken(pat.id)}
                >
                  {#if revokingId === pat.id}
                    <Loader2 class="size-4 animate-spin" />
                    Revoking...
                  {:else}
                    Revoke
                  {/if}
                </Button>
              </div>
            </CardContent>
          </Card>
        {/each}
      </div>
    {/if}
  </CardContent>
</Card>

<Modal
  bind:open={modalOpen}
  title={createdRawToken ? "Token Created Successfully" : "Create New Access Token"}
  description={createdRawToken ? "Make sure to copy your personal access token now. You won't be able to see it again." : "Generate a new token for authentication."}
  width="max-w-[450px]"
  onclose={handleCloseModal}
>
  {#if createdRawToken}
    <div class="flex flex-col gap-6 pt-2">
      <div class="flex items-start gap-3 rounded-lg border border-yellow-500/20 bg-yellow-500/5 p-4 text-yellow-600 dark:text-yellow-500">
        <TriangleAlert class="size-5 shrink-0 mt-0.5" />
        <div class="text-xs leading-relaxed">
          <span class="font-semibold">Warning:</span> Copy this token now. It will not be shown to you again for security reasons. If you lose it, you will have to create a new one.
        </div>
      </div>

      <div class="flex items-center gap-2 rounded-lg border border-border bg-muted/50 p-3">
        <code class="flex-1 select-all break-all font-mono text-sm">{createdRawToken}</code>
        <Button
          size="icon"
          variant="ghost"
          onclick={() => void copyToClipboard(createdRawToken!)}
          aria-label="Copy token"
        >
          {#if copied}
            <Check class="size-4 text-green-500" />
          {:else}
            <Copy class="size-4" />
          {/if}
        </Button>
      </div>

      <div class="flex justify-end">
        <Button onclick={handleCloseModal}>Close</Button>
      </div>
    </div>
  {:else}
    <form class="flex flex-col gap-6 pt-2" onsubmit={handleCreateToken}>
      <FieldGroup>
        <Field label="Token Name" forId="token_name" description="Give your token a descriptive name (e.g. My CLI).">
          <Input id="token_name" bind:value={tokenName} placeholder="my-cli" required autocomplete="off" />
        </Field>
      </FieldGroup>

      <div class="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
        <Button variant="outline" type="button" onclick={handleCloseModal}>Cancel</Button>
        <Button type="submit" disabled={creating || !tokenName.trim()}>
          {#if creating}
            <Loader2 class="size-4 animate-spin mr-2" />
            Creating...
          {:else}
            Create Token
          {/if}
        </Button>
      </div>
    </form>
  {/if}
</Modal>
