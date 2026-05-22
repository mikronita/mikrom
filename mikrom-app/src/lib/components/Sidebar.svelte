<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import {
    Boxes,
    Database,
    ChevronsUpDown,
    HardDrive,
    LayoutDashboard,
    LogOut,
    Network,
    Settings,
  } from "lucide-svelte";
  import { logout } from "$lib/auth";
  import { Avatar, AvatarFallback } from "$lib/components";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
  import * as Tooltip from "$lib/components/ui/tooltip/index.js";
  import { profile } from "$lib/stores/profile";
  import { cn } from "$lib/utils";

  const SIDEBAR_COOKIE_NAME = "sidebar_state";
  const SIDEBAR_COOKIE_MAX_AGE = 60 * 60 * 24 * 7;

  export let collapsed = false;

  const nav = [
    { href: "/", label: "Dashboard", icon: LayoutDashboard },
    { href: "/apps", label: "Applications", icon: Boxes },
    { href: "/databases", label: "Databases", icon: Database },
    { href: "/networking", label: "Networking", icon: Network },
    { href: "/storage", label: "Storage", icon: HardDrive },
    { href: "/settings", label: "Settings", icon: Settings },
  ];

  function initials() {
    if (!$profile) return "U";
    const full =
      `${$profile.first_name?.[0] || ""}${$profile.last_name?.[0] || ""}`.toUpperCase();
    return full || $profile.email?.[0]?.toUpperCase() || "U";
  }

  function displayName() {
    if (!$profile) return "";
    if ($profile.first_name && $profile.last_name)
      return `${$profile.first_name} ${$profile.last_name}`;
    return $profile.email.split("@")[0] || "User";
  }

  function persistCollapsedState(nextCollapsed: boolean) {
    collapsed = nextCollapsed;
    document.cookie = `${SIDEBAR_COOKIE_NAME}=${nextCollapsed}; path=/; max-age=${SIDEBAR_COOKIE_MAX_AGE}`;
  }

  function toggleCollapsed() {
    persistCollapsedState(!collapsed);
  }

  $: pathname = $page.url.pathname;

  onMount(() => {
    const persisted = document.cookie
      .split("; ")
      .find((row) => row.startsWith(`${SIDEBAR_COOKIE_NAME}=`))
      ?.split("=")[1];

    if (persisted) {
      collapsed = persisted === "true";
    }

    const handleKeydown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "b") {
        event.preventDefault();
        toggleCollapsed();
      }
    };

    window.addEventListener("keydown", handleKeydown);

    return () => {
      window.removeEventListener("keydown", handleKeydown);
    };
  });
</script>

<aside
  class="group peer hidden shrink-0 text-sidebar-foreground md:block"
  data-state={collapsed ? "collapsed" : "expanded"}
  data-collapsible="icon"
  data-variant="sidebar"
  data-side="left"
>
  <div
    class="relative hidden h-svh bg-transparent transition-[width] duration-200 ease-linear md:block"
    style={`width: ${collapsed ? "3rem" : "14rem"}`}
  >
    <div
      class="fixed inset-y-0 left-0 z-10 hidden h-svh transition-[left,right,width] duration-200 ease-linear md:flex"
      style={`width: ${collapsed ? "3rem" : "14rem"}`}
    >
      <div class="flex h-full w-full flex-col bg-card text-card-foreground">
        <div class="flex h-14 items-center border-b border-border p-2">
          <a
            href="/"
            class={cn(
              "flex h-10 w-full items-center rounded-md px-2 transition-colors hover:bg-muted",
              collapsed ? "justify-center gap-0" : "gap-3"
            )}
          >
            <div
              class="flex size-8 shrink-0 items-center justify-center rounded-md border border-border bg-background text-foreground"
            >
              <Boxes />
            </div>
            {#if !collapsed}
              <div class="flex flex-col overflow-hidden">
                <span
                  class="whitespace-nowrap text-sm font-semibold leading-none"
                  >Mikrom</span
                >
                <span class="mt-1 text-xs text-muted-foreground"
                  >Cloud Platform</span
                >
              </div>
            {/if}
          </a>
        </div>

        <div class="flex-1 overflow-y-auto p-2">
          {#if !collapsed}
            <div
              class="px-2 py-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground"
            >
              Workspace
            </div>
          {/if}
          <nav class="flex flex-col gap-1">
            {#each nav as item}
              <Tooltip.Provider>
                <Tooltip.Root delayDuration={0}>
                  <Tooltip.Trigger>
                    {#snippet child({ props })}
                      <a
                        href={item.href}
                        {...props}
                        class={cn(
                          "flex h-9 items-center rounded-md px-2 text-sm outline-none transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
                          item.href === "/"
                            ? pathname === item.href
                              ? "bg-muted text-foreground"
                              : "text-foreground"
                            : pathname.startsWith(item.href)
                              ? "bg-muted text-foreground"
                              : "text-foreground",
                          collapsed ? "justify-center" : "gap-2"
                        )}
                      >
                        <svelte:component this={item.icon} class="size-4 shrink-0" />
                        {#if !collapsed}
                          <span class="truncate">{item.label}</span>
                        {/if}
                      </a>
                    {/snippet}
                  </Tooltip.Trigger>
                  {#if collapsed}
                    <Tooltip.Content side="right" align="center">
                      {item.label}
                    </Tooltip.Content>
                  {/if}
                </Tooltip.Root>
              </Tooltip.Provider>
            {/each}
          </nav>
        </div>

        <div class="relative border-t border-border p-2">
          <DropdownMenu.Root>
            <DropdownMenu.Trigger>
              {#snippet child({ props })}
                <button
                  type="button"
                  {...props}
                  class={cn(
                    "flex h-12 w-full items-center rounded-md p-2 text-left text-sm outline-none transition-colors hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring data-[state=open]:bg-muted data-[state=open]:text-foreground",
                    collapsed ? "justify-center gap-0" : "gap-3"
                  )}
                  aria-label="User menu"
                >
                  <Avatar class="size-8 shrink-0 rounded-md">
                    <AvatarFallback
                      class="rounded-md text-xs font-medium text-foreground"
                    >
                      {initials()}
                    </AvatarFallback>
                  </Avatar>
                  {#if !collapsed}
                    <div class="grid flex-1 text-left text-sm leading-tight">
                      <span class="truncate font-medium">{displayName()}</span>
                      <span class="truncate text-xs text-muted-foreground"
                        >{$profile?.email || ""}</span
                      >
                    </div>
                    <ChevronsUpDown
                      class="ml-auto size-4 shrink-0 text-muted-foreground"
                    />
                  {/if}
                </button>
              {/snippet}
            </DropdownMenu.Trigger>
            <DropdownMenu.Content class="w-56" side="right" align="end" sideOffset={8}>
              <DropdownMenu.Label class="font-normal">
                <div class="flex items-center gap-2 px-1 py-1.5 text-left text-sm">
                  <Avatar class="size-8 rounded-md">
                    <AvatarFallback class="rounded-md text-xs font-medium text-foreground">
                      {initials()}
                    </AvatarFallback>
                  </Avatar>
                  <div class="grid flex-1 text-left text-sm leading-tight">
                    <span class="truncate font-semibold">{displayName()}</span>
                    <span class="truncate text-xs text-muted-foreground">{$profile?.email || ""}</span>
                  </div>
                </div>
              </DropdownMenu.Label>
              <DropdownMenu.Separator />
              <DropdownMenu.Group>
                <DropdownMenu.Item>
                  {#snippet child({ props })}
                    <a href="/settings" {...props} class="flex w-full items-center">
                      <Settings class="mr-2 size-4" />
                      <span>Settings</span>
                    </a>
                  {/snippet}
                </DropdownMenu.Item>
              </DropdownMenu.Group>
              <DropdownMenu.Separator />
              <DropdownMenu.Item onSelect={() => logout()} class="text-destructive">
                <LogOut class="mr-2 size-4" />
                <span>Sign out</span>
              </DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Root>
        </div>

        <button
          type="button"
          aria-label="Toggle Sidebar"
          title="Toggle Sidebar"
          class="absolute inset-y-0 z-20 hidden w-4 -translate-x-1/2 transition-all ease-linear after:absolute after:inset-y-0 after:left-1/2 after:w-[2px] hover:after:bg-border group-data-[side=left]:-right-4 sm:flex"
          on:click={toggleCollapsed}
        ></button>
      </div>
    </div>
  </div>
</aside>
