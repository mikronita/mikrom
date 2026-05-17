<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { isAuthenticated } from "$lib/auth";

  let checking = true;

  onMount(() => {
    if (!isAuthenticated()) {
      goto("/auth/login");
      return;
    }

    checking = false;
  });
</script>

{#if checking}
  <div class="flex min-h-screen items-center justify-center bg-background text-muted-foreground">
    <div class="flex items-center gap-3 rounded-md border bg-card px-4 py-3 shadow-sm">
      <div class="size-3 animate-pulse rounded-full bg-primary"></div>
      <span>Loading workspace...</span>
    </div>
  </div>
{:else}
  <slot />
{/if}
