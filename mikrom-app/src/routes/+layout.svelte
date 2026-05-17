<script lang="ts">
  import "../app.css";
  import ToastViewport from "$lib/components/ToastViewport.svelte";
  import { initTheme } from "$lib/theme";

  initTheme();
</script>

<svelte:head>
  <title>Mikrom</title>
  <meta name="description" content="Micromobility management platform" />
</svelte:head>

<slot />
<ToastViewport />
