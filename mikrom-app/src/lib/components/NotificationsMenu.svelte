<script lang="ts">
  import { browser } from "$app/environment";
  import { goto } from "$app/navigation";
    import Bell from "@lucide/svelte/icons/bell";
  import CheckCheck from "@lucide/svelte/icons/check-check";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { Badge } from "$lib/components";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
  import { cn } from "$lib/utils";
  import {
    clearWorkspaceEventNotifications,
    dismissWorkspaceEventNotification,
    notificationsError,
    notificationsHasMore,
    notificationsLoading,
    notificationsFeed,
    notificationsUnreadCountTotal,
    type NotificationsFeedItem,
    loadMoreNotifications,
    readAllNotifications,
    readNotificationById,
    refreshNotifications,
    useNotificationsBootstrap,
  } from "$lib/stores/notifications";
  import { activeProjectSlugStore } from "$lib/stores/projects";

  useNotificationsBootstrap();

  let {
    className = "",
  } = $props<{
    className?: string;
  }>();

  let unreadOnly = $state(false);
  let showLiveUpdates = $state(true);
  let notificationsScrollContainer = $state<HTMLElement | null>(null);
  type DropdownWidthMode = "compact" | "default" | "wide";
  let dropdownWidthMode = $state<DropdownWidthMode>("default");
  const unreadOnlyStateKey = $derived(
    `mikrom_notifications_unread_only:${$activeProjectSlugStore ?? "global"}`
  );
  const liveUpdatesStateKey = $derived(
    `mikrom_notifications_live_updates_visible:${$activeProjectSlugStore ?? "global"}`
  );
  const scrollStateKey = $derived(
    `mikrom_notifications_scroll_top:${$activeProjectSlugStore ?? "global"}`
  );
  const widthStateKey = $derived(
    `mikrom_notifications_width_mode:${$activeProjectSlugStore ?? "global"}`
  );
  const dropdownWidthClass = $derived(
    dropdownWidthMode === "compact"
      ? "w-[18rem]"
      : dropdownWidthMode === "wide"
        ? "w-[28rem]"
        : "w-[22rem]"
  );
  const liveNotifications = $derived($notificationsFeed.filter((notification) => notification.source === "workspace_event"));
  const persistedNotifications = $derived($notificationsFeed.filter((notification) => notification.source !== "workspace_event"));

  function formatRelativeTime(value: string) {
    const date = new Date(value);
    const diffMs = Date.now() - date.getTime();
    const minutes = Math.max(1, Math.round(diffMs / 60000));

    if (minutes < 60) return `${minutes}m ago`;
    const hours = Math.round(minutes / 60);
    if (hours < 24) return `${hours}h ago`;
    const days = Math.round(hours / 24);
    return `${days}d ago`;
  }

  async function openNotification(notification: NotificationsFeedItem) {
    if (notification.source === "workspace_event") {
      dismissWorkspaceEventNotification(notification.id);
    } else {
      await readNotificationById(notification.id);
    }

    if (notification.route) {
      await goto(notification.route);
    }
  }

  async function setUnreadOnly(nextUnreadOnly: boolean) {
    unreadOnly = nextUnreadOnly;
    if (browser) {
      window.localStorage.setItem(unreadOnlyStateKey, String(nextUnreadOnly));
    }
    await refreshNotifications({ unreadOnly: nextUnreadOnly, reset: true });
  }

  function clearLiveUpdates() {
    clearWorkspaceEventNotifications();
  }

  function persistDropdownWidthMode(nextMode: DropdownWidthMode) {
    dropdownWidthMode = nextMode;
    if (!browser) return;
    window.localStorage.setItem(widthStateKey, nextMode);
  }

  function persistScrollState() {
    if (!browser || !notificationsScrollContainer) return;
    window.localStorage.setItem(scrollStateKey, String(notificationsScrollContainer.scrollTop));
  }

  function persistLiveUpdatesState(nextVisible: boolean) {
    showLiveUpdates = nextVisible;
    if (!browser) return;
    window.localStorage.setItem(liveUpdatesStateKey, String(nextVisible));
  }

  $effect(() => {
    if (!browser) return;

    const persistedUnreadOnly = window.localStorage.getItem(unreadOnlyStateKey);
    if (persistedUnreadOnly === "true" || persistedUnreadOnly === "false") {
      unreadOnly = persistedUnreadOnly === "true";
    } else {
      unreadOnly = false;
    }

    const persisted = window.localStorage.getItem(liveUpdatesStateKey);
    if (persisted === "true" || persisted === "false") {
      showLiveUpdates = persisted === "true";
    } else {
      showLiveUpdates = true;
    }

    const persistedWidth = window.localStorage.getItem(widthStateKey);
    if (persistedWidth === "compact" || persistedWidth === "default" || persistedWidth === "wide") {
      dropdownWidthMode = persistedWidth;
    } else {
      dropdownWidthMode = "default";
    }

    if (notificationsScrollContainer) {
      const persistedScroll = window.localStorage.getItem(scrollStateKey);
      const nextScrollTop = persistedScroll ? Number(persistedScroll) : 0;
      notificationsScrollContainer.scrollTop = Number.isFinite(nextScrollTop) ? nextScrollTop : 0;
    }
  });
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger>
    {#snippet child({ props })}
      <button
        type="button"
        {...props}
        class={cn(
          "relative flex size-9 items-center justify-center rounded-md hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          className,
        )}
        aria-label="Notifications"
      >
        <Bell class="size-4" />
        {#if $notificationsUnreadCountTotal > 0}
          <span class="absolute -right-0.5 -top-0.5 inline-flex min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[10px] font-semibold leading-4 text-destructive-foreground">
            {Math.min($notificationsUnreadCountTotal, 99)}
          </span>
        {/if}
      </button>
    {/snippet}
  </DropdownMenu.Trigger>

  <DropdownMenu.Content class={cn(dropdownWidthClass, "max-w-[calc(100vw-1rem)] p-0")} align="end" sideOffset={10} data-testid="notifications-dropdown">
    <div class="flex flex-col gap-3 border-b border-border px-4 py-3">
      <div class="flex items-center justify-between gap-3">
        <div class="min-w-0">
          <p class="text-sm font-semibold">Notifications</p>
          <p class="text-xs text-muted-foreground">{$notificationsUnreadCountTotal} unread</p>
        </div>
        <button
          type="button"
          class="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium text-muted-foreground hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
          onclick={() => void readAllNotifications()}
          disabled={$notificationsUnreadCountTotal === 0}
        >
          <CheckCheck class="size-3.5" />
          Mark all read
        </button>
      </div>
      <div class="inline-flex w-fit rounded-md border border-border bg-muted p-1">
        <button
          type="button"
          class={cn(
            "rounded-sm px-2 py-1 text-xs font-medium transition-colors",
            !unreadOnly ? "bg-background text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground",
          )}
          onclick={() => void setUnreadOnly(false)}
        >
          All
        </button>
        <button
          type="button"
          class={cn(
            "rounded-sm px-2 py-1 text-xs font-medium transition-colors",
            unreadOnly ? "bg-background text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground",
          )}
          onclick={() => void setUnreadOnly(true)}
        >
          Unread only
        </button>
      </div>
      <div class="flex items-center justify-between gap-2">
        <p class="text-[11px] uppercase tracking-wide text-muted-foreground">Width</p>
        <div class="inline-flex rounded-md border border-border bg-muted p-1">
          <button
            type="button"
            class={cn(
              "rounded-sm px-2 py-1 text-xs font-medium transition-colors",
              dropdownWidthMode === "compact" ? "bg-background text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground",
            )}
            onclick={() => persistDropdownWidthMode("compact")}
          >
            Compact
          </button>
          <button
            type="button"
            class={cn(
              "rounded-sm px-2 py-1 text-xs font-medium transition-colors",
              dropdownWidthMode === "default" ? "bg-background text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground",
            )}
            onclick={() => persistDropdownWidthMode("default")}
          >
            Default
          </button>
          <button
            type="button"
            class={cn(
              "rounded-sm px-2 py-1 text-xs font-medium transition-colors",
              dropdownWidthMode === "wide" ? "bg-background text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground",
            )}
            onclick={() => persistDropdownWidthMode("wide")}
          >
            Wide
          </button>
        </div>
      </div>
    </div>

    {#if $notificationsError}
      <div class="mx-4 mt-3 rounded-md border border-destructive/20 bg-destructive/5 px-3 py-2 text-xs text-destructive">
        {$notificationsError}
      </div>
    {/if}

    <div
      bind:this={notificationsScrollContainer}
      data-testid="notifications-scroll-container"
      class="max-h-[28rem] overflow-auto p-2"
      onscroll={persistScrollState}
    >
      {#if liveNotifications.length > 0}
        <div class="mb-3 rounded-lg border border-amber-500/20 bg-amber-500/[0.04] p-2">
          <div class="mb-2 flex items-center justify-between gap-2 px-2">
            <div class="min-w-0">
              <p class="text-xs font-semibold uppercase tracking-wide text-amber-700">Live updates</p>
              <p class="text-[11px] text-muted-foreground">Recent SSE events from your workspace.</p>
            </div>
            <div class="flex items-center gap-2">
              <Badge variant="outline" class="border-amber-500/25 bg-amber-500/10 text-amber-700">
                {liveNotifications.length}
              </Badge>
              <button
                type="button"
                class="inline-flex items-center rounded-md px-2 py-1 text-xs font-medium text-amber-700 hover:bg-amber-500/10"
                onclick={() => persistLiveUpdatesState(!showLiveUpdates)}
              >
                {showLiveUpdates ? "Hide" : "Show"}
              </button>
              <button
                type="button"
                class="inline-flex items-center rounded-md px-2 py-1 text-xs font-medium text-amber-700 hover:bg-amber-500/10 disabled:pointer-events-none disabled:opacity-50"
                onclick={clearLiveUpdates}
                disabled={liveNotifications.length === 0}
              >
                Clear
              </button>
            </div>
          </div>
          {#if showLiveUpdates}
            <div class="flex flex-col gap-2">
              {#each liveNotifications as notification}
                <button
                  type="button"
                  class={cn(
                    "w-full rounded-lg border border-amber-500/20 bg-amber-500/[0.06] px-3 py-3 text-left transition-colors hover:bg-amber-500/[0.1]",
                  )}
                  onclick={() => void openNotification(notification)}
                >
                  <div class="flex items-start gap-3">
                    <div
                      class="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md border border-amber-500/25 bg-amber-500/10 text-amber-600"
                    >
                      <Bell class="size-4" />
                    </div>
                    <div class="min-w-0 flex-1">
                      <div class="flex items-center justify-between gap-3">
                        <div class="flex min-w-0 items-center gap-2">
                          <span class="inline-flex size-1.5 shrink-0 rounded-full bg-amber-500 animate-pulse"></span>
                          <p class="truncate text-sm font-medium text-foreground">{notification.title}</p>
                          <Badge variant="outline" class="border-amber-500/25 bg-amber-500/10 text-amber-700">
                            Live
                          </Badge>
                        </div>
                        <span class="shrink-0 text-[11px] text-muted-foreground">
                          {formatRelativeTime(notification.created_at)}
                        </span>
                      </div>
                      <p class="mt-1 line-clamp-2 text-sm text-muted-foreground">{notification.body}</p>
                      <div class="mt-2 flex items-center justify-between gap-2">
                        <span class="truncate text-[11px] uppercase tracking-wide text-muted-foreground">
                          {notification.entity_name || notification.kind.replaceAll("_", " ")}
                        </span>
                        <div class="flex items-center gap-2">
                          {#if !notification.is_read}
                            <Badge variant="outline" class="border-primary/20 bg-primary/10 text-primary">New</Badge>
                          {/if}
                        </div>
                      </div>
                    </div>
                  </div>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      {#if $notificationsLoading && persistedNotifications.length === 0 && liveNotifications.length === 0}
        <div class="flex items-center gap-2 px-3 py-6 text-sm text-muted-foreground">
          <LoaderCircle class="size-4 animate-spin" />
          Loading notifications
        </div>
      {:else if persistedNotifications.length === 0 && liveNotifications.length === 0}
        <div class="flex flex-col items-center gap-2 px-4 py-10 text-center text-sm text-muted-foreground">
          <CircleAlert class="size-5" />
          <p>No notifications yet</p>
          <p class="max-w-56 text-xs">Activity from your project will appear here in real time.</p>
        </div>
      {:else}
        <div class="flex flex-col gap-2">
          {#each persistedNotifications as notification}
            <button
              type="button"
              class={cn(
                "w-full rounded-lg border px-3 py-3 text-left transition-colors hover:bg-muted/80",
                notification.is_read ? "border-border bg-background" : "border-primary/20 bg-primary/5",
              )}
              onclick={() => void openNotification(notification)}
            >
              <div class="flex items-start gap-3">
                <div
                  class={cn(
                    "mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md border",
                    notification.is_read
                      ? "border-border bg-background text-muted-foreground"
                      : "border-primary/20 bg-primary/10 text-primary",
                  )}
                >
                  <Bell class="size-4" />
                </div>
                <div class="min-w-0 flex-1">
                  <div class="flex items-center justify-between gap-3">
                    <p class="truncate text-sm font-medium text-foreground">{notification.title}</p>
                    <span class="shrink-0 text-[11px] text-muted-foreground">
                      {formatRelativeTime(notification.created_at)}
                    </span>
                  </div>
                  <p class="mt-1 line-clamp-2 text-sm text-muted-foreground">{notification.body}</p>
                  <div class="mt-2 flex items-center justify-between gap-2">
                    <span class="truncate text-[11px] uppercase tracking-wide text-muted-foreground">
                      {notification.entity_name || notification.kind.replaceAll("_", " ")}
                    </span>
                    <div class="flex items-center gap-2">
                      {#if !notification.is_read}
                        <Badge variant="outline" class="border-primary/20 bg-primary/10 text-primary">New</Badge>
                      {/if}
                    </div>
                  </div>
                </div>
              </div>
            </button>
          {/each}
          {#if $notificationsHasMore}
            <button
              type="button"
              class="mt-1 inline-flex items-center justify-center rounded-md border border-border px-3 py-2 text-sm font-medium text-foreground transition-colors hover:bg-muted disabled:pointer-events-none disabled:opacity-50"
              onclick={() => void loadMoreNotifications()}
              disabled={$notificationsLoading}
            >
              {$notificationsLoading ? "Loading more..." : "Load more"}
            </button>
          {/if}
        </div>
      {/if}
    </div>
  </DropdownMenu.Content>
</DropdownMenu.Root>
