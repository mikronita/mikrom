<script lang="ts">
  import { goto } from "$app/navigation";
  import { Bell, CheckCheck, CircleAlert, LoaderCircle } from "lucide-svelte";
  import { Badge } from "$lib/components";
  import * as DropdownMenu from "$lib/components/ui/dropdown-menu/index.js";
  import { cn } from "$lib/utils";
  import {
    notificationsError,
    notificationsHasMore,
    notificationsLoading,
    notificationsUnreadCount,
    notificationsStore,
    loadMoreNotifications,
    readAllNotifications,
    readNotificationById,
    refreshNotifications,
    useNotificationsBootstrap,
  } from "$lib/stores/notifications";

  useNotificationsBootstrap();

  let {
    className = "",
  } = $props<{
    className?: string;
  }>();

  let unreadOnly = $state(false);

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

  async function openNotification(route: string, notificationId: string) {
    await readNotificationById(notificationId);
    await goto(route);
  }

  async function setUnreadOnly(nextUnreadOnly: boolean) {
    unreadOnly = nextUnreadOnly;
    await refreshNotifications({ unreadOnly: nextUnreadOnly, reset: true });
  }
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
        {#if $notificationsUnreadCount > 0}
          <span class="absolute -right-0.5 -top-0.5 inline-flex min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[10px] font-semibold leading-4 text-destructive-foreground">
            {Math.min($notificationsUnreadCount, 99)}
          </span>
        {/if}
      </button>
    {/snippet}
  </DropdownMenu.Trigger>

  <DropdownMenu.Content class="w-[22rem] max-w-[calc(100vw-1rem)] p-0" align="end" sideOffset={10}>
    <div class="flex flex-col gap-3 border-b border-border px-4 py-3">
      <div class="flex items-center justify-between gap-3">
        <div class="min-w-0">
          <p class="text-sm font-semibold">Notifications</p>
          <p class="text-xs text-muted-foreground">{$notificationsUnreadCount} unread</p>
        </div>
        <button
          type="button"
          class="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium text-muted-foreground hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
          onclick={() => void readAllNotifications()}
          disabled={$notificationsUnreadCount === 0}
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
    </div>

    {#if $notificationsError}
      <div class="mx-4 mt-3 rounded-md border border-destructive/20 bg-destructive/5 px-3 py-2 text-xs text-destructive">
        {$notificationsError}
      </div>
    {/if}

    <div class="max-h-[28rem] overflow-auto p-2">
      {#if $notificationsLoading && $notificationsStore.length === 0}
        <div class="flex items-center gap-2 px-3 py-6 text-sm text-muted-foreground">
          <LoaderCircle class="size-4 animate-spin" />
          Loading notifications
        </div>
      {:else if $notificationsStore.length === 0}
        <div class="flex flex-col items-center gap-2 px-4 py-10 text-center text-sm text-muted-foreground">
          <CircleAlert class="size-5" />
          <p>No notifications yet</p>
          <p class="max-w-56 text-xs">Activity from your project will appear here in real time.</p>
        </div>
      {:else}
        <div class="flex flex-col gap-2">
          {#each $notificationsStore as notification}
            <button
              type="button"
              class={cn(
                "w-full rounded-lg border px-3 py-3 text-left transition-colors hover:bg-muted/80",
                notification.is_read ? "border-border bg-background" : "border-primary/20 bg-primary/5",
              )}
              onclick={() => void openNotification(notification.route, notification.id)}
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
                    {#if !notification.is_read}
                      <Badge variant="outline" class="border-primary/20 bg-primary/10 text-primary">New</Badge>
                    {/if}
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
