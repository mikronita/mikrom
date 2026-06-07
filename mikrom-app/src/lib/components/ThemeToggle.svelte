<script lang="ts">
  import { Moon, Sun } from "lucide-svelte";
  import { setTheme, themeStore } from "$lib/theme";

  function toggle() {
    setTheme($themeStore === "dark" ? "light" : "dark");
  }
</script>

<button
  type="button"
  class="relative inline-flex h-9 w-9 items-center justify-center rounded-md border border-transparent bg-transparent text-foreground transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
  aria-label="Toggle theme"
  onclick={toggle}
>
  {#if $themeStore === "dark"}
    <Sun class="size-4" />
  {:else}
    <Moon class="size-4" />
  {/if}
</button>
