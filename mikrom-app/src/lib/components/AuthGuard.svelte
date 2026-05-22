<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { Progress } from "$lib/components";
  import { isAuthenticated } from "$lib/auth";

  let { children } = $props();
  let checking = $state(true);

  onMount(() => {
    if (!isAuthenticated()) {
      goto("/auth/login");
      return;
    }

    checking = false;
  });
</script>

{#if checking}
  <div class="flex min-h-screen items-center justify-center bg-background px-6">
    <Progress class="w-56 sm:w-72" />
  </div>
{:else}
  {@render children?.()}
{/if}
