<script lang="ts">
  import { page } from "$app/stores";
  import { Bell, PanelLeft, Search } from "lucide-svelte";
  import AuthGuard from "$lib/components/AuthGuard.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ThemeToggle from "$lib/components/ThemeToggle.svelte";
  import { useProfileBootstrap } from "$lib/stores/profile";

  useProfileBootstrap();

  const segmentName = (segment: string) => decodeURIComponent(segment).replace(/^\w/, (c) => c.toUpperCase());

  let pathParts: string[] = [];
  let sidebarCollapsed = false;
  $: pathParts = $page.url.pathname.split("/").filter(Boolean);

  function toggleSidebar() {
    sidebarCollapsed = !sidebarCollapsed;
    document.cookie = `sidebar_state=${sidebarCollapsed}; path=/; max-age=${60 * 60 * 24 * 7}`;
  }
</script>

<AuthGuard>
  <div class="flex min-h-screen bg-background">
    <Sidebar bind:collapsed={sidebarCollapsed} />
    <div class="flex min-w-0 flex-1 flex-col">
      <header class="sticky top-0 z-10 hidden h-14 shrink-0 items-center gap-3 border-b border-border bg-background/95 px-3 backdrop-blur supports-[backdrop-filter]:bg-background/80 md:flex md:px-4">
        <button
          type="button"
          class="flex size-9 items-center justify-center rounded-md hover:bg-muted"
          aria-label="Toggle Sidebar"
          title="Toggle Sidebar"
          on:click={toggleSidebar}
        >
          <PanelLeft class="size-4" />
        </button>
        <div class="flex min-w-0 flex-1 items-center gap-2 overflow-hidden text-sm text-muted-foreground">
          <a href="/" class="hidden font-medium text-foreground md:block">Home</a>
          {#if pathParts.length}
            {#each pathParts as part, index}
              <span class="hidden md:block">/</span>
              <span class={index === pathParts.length - 1 ? "max-w-[140px] truncate font-medium text-foreground sm:max-w-none" : "hidden sm:block"}>
                {segmentName(part)}
              </span>
            {/each}
          {/if}
        </div>
        <div class="hidden w-72 lg:block">
          <div class="flex h-9 items-center gap-2 rounded-md border border-border bg-background px-3 text-sm text-muted-foreground">
            <Search class="size-4" />
            <span>Search or jump to...</span>
          </div>
        </div>
        <button class="flex size-9 items-center justify-center rounded-md hover:bg-muted" aria-label="Notifications" type="button">
          <Bell class="size-4" />
        </button>
        <ThemeToggle />
      </header>

      <main class="flex-1 p-3 md:p-4">
        <div class="mx-auto w-full max-w-7xl">
          <slot />
        </div>
      </main>
    </div>
  </div>
</AuthGuard>
