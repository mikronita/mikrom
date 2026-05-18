<script lang="ts">
  import "../app.css";
  import { Toaster } from "svelte-sonner";
  import { initTheme } from "$lib/theme";
  import { onMount } from "svelte";
  import { initWorkspaceSSE } from "$lib/stores/workspace";
  import { initVmsSSE } from "$lib/stores/vms";

  initTheme();

  onMount(() => {
    initWorkspaceSSE();
    initVmsSSE();
  });
</script>

<svelte:head>
  <title>Mikrom</title>
  <meta name="description" content="Micromobility management platform" />
</svelte:head>

<slot />
<Toaster position="bottom-right" richColors />
