<script lang="ts">
  import { onMount } from "svelte";
  import { Moon, Sun } from "lucide-svelte";
  import { getTheme, setTheme } from "$lib/theme";

  let theme = getTheme();

  onMount(() => {
    const syncTheme = () => {
      theme = getTheme();
    };

    syncTheme();
    window.addEventListener("storage", syncTheme);
    window.addEventListener("mikrom-theme-change", syncTheme);

    return () => {
      window.removeEventListener("storage", syncTheme);
      window.removeEventListener("mikrom-theme-change", syncTheme);
    };
  });

  function toggle() {
    theme = theme === "dark" ? "light" : "dark";
    setTheme(theme as "light" | "dark");
  }
</script>

<button
  type="button"
  class="relative inline-flex h-9 w-9 items-center justify-center rounded-md border border-transparent bg-transparent text-foreground transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
  aria-label="Toggle theme"
  on:click={toggle}
>
  <Moon class={`size-4 transition-all ${theme === "dark" ? "rotate-0 scale-100" : "rotate-90 scale-0"}`} />
  <Sun class={`absolute size-4 transition-all ${theme === "dark" ? "-rotate-90 scale-0" : "rotate-0 scale-100"}`} />
</button>
