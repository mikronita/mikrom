<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/stores";
    import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Terminal from "@lucide/svelte/icons/terminal";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components";
  import { getToken } from "$lib/auth";
  import { type LogLine, watchAppLogsSSE } from "$lib/api";
  import { goto } from "$app/navigation";

  let appName = $derived(decodeURIComponent($page.params.appName ?? ""));
  let jobId = $derived(decodeURIComponent($page.params.jobId ?? ""));
  let logs = $state<LogLine[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let container = $state<HTMLDivElement | null>(null);

  function reconnect() {
    logs = [];
    loading = true;
    error = null;
  }

  onMount(() => {
    const token = getToken();
    if (!token || !appName) return;

    const cleanup = watchAppLogsSSE(token, appName, (payload) => {
      const lines = Array.isArray(payload) ? payload : [payload];
      logs = [...logs, ...lines].slice(-1000);
      loading = false;
    });

    return cleanup;
  });

  $effect(() => {
    if (!container || logs.length === 0) return;
    container.scrollTop = container.scrollHeight;
  });
</script>

<svelte:head>
  <title>Deployment Logs</title>
</svelte:head>

<div class="mx-auto flex max-w-6xl flex-col gap-6 p-6">
  <div class="flex items-center justify-between gap-3">
    <div>
      <h1 class="text-2xl font-semibold tracking-tight">Deployment logs</h1>
      <p class="text-sm text-muted-foreground">{appName} · {jobId}</p>
    </div>
    <Button variant="outline" onclick={() => goto(`/apps/${encodeURIComponent(appName)}`)}>
      <ArrowLeft class="size-4" />
      Back to app
    </Button>
  </div>

  <Card>
    <CardHeader class="flex flex-row items-center justify-between gap-3 space-y-0">
      <div>
        <CardTitle class="flex items-center gap-2 text-lg">
          <Terminal class="size-5" />
          Live stream
        </CardTitle>
        <CardDescription>Full deployment log stream.</CardDescription>
      </div>
      <Button variant="outline" size="sm" onclick={() => (logs = [])}>
        <RefreshCw class="size-4" />
        Clear
      </Button>
    </CardHeader>
    <CardContent>
      {#if loading && logs.length === 0}
        <div class="rounded-xl border border-dashed border-border bg-background/60 p-6 text-sm text-muted-foreground">
          Waiting for the deployment log stream to connect...
        </div>
      {:else if error}
        <div class="rounded-xl border border-destructive/30 bg-destructive/5 p-6 text-sm text-destructive">
          {error}
        </div>
      {:else if logs.length === 0}
        <div class="rounded-xl border border-dashed border-border bg-background/60 p-6 text-sm text-muted-foreground">
          <p>No logs received yet for this deployment.</p>
          <div class="mt-4 flex flex-wrap gap-2">
            <Button variant="outline" size="sm" onclick={reconnect}>
              <RefreshCw class="size-4" />
              Reconnect stream
            </Button>
            <Button variant="outline" size="sm" onclick={() => goto(`/apps/${encodeURIComponent(appName)}`)}>
              <ArrowLeft class="size-4" />
              Back to app
            </Button>
          </div>
        </div>
      {:else}
        <div bind:this={container} class="max-h-[36rem] overflow-auto rounded-xl border border-border bg-[#0b1020] p-4 font-mono text-xs leading-5 text-slate-100">
          {#each logs as log}
            <div class="flex gap-3 border-b border-white/5 py-1 last:border-b-0">
              <span class="shrink-0 text-slate-400">{new Date(log.timestamp).toLocaleTimeString()}</span>
              <span class="min-w-0 flex-1 whitespace-pre-wrap break-words">{log.line}</span>
            </div>
          {/each}
        </div>
      {/if}
    </CardContent>
  </Card>
</div>
