<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import Progress from "$lib/components/Progress.svelte";
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
  <div class="flex min-h-screen items-center justify-center bg-background px-6">
    <Progress className="w-56 sm:w-72" />
  </div>
{:else}
  <slot />
{/if}
