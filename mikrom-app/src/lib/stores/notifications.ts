import { onMount } from "svelte";
import { derived, get } from "svelte/store";
import { writable } from "svelte/store";
import {
  type WorkspaceEvent,
  getNotifications,
  markAllNotificationsRead,
  markNotificationRead,
  type WorkspaceNotification,
} from "$lib/api";
import { getToken } from "$lib/auth";

const DEFAULT_PAGE_SIZE = 10;
const WORKSPACE_EVENT_DEDUP_WINDOW_MS = 5 * 60 * 1000;
export type NotificationSource = "backend" | "workspace_event";
export type NotificationsFeedItem = WorkspaceNotification & {
  source: NotificationSource;
};

export const notificationsStore = writable<WorkspaceNotification[]>([]);
const workspaceEventNotificationsStore = writable<WorkspaceNotification[]>([]);
export const notificationsUnreadCount = writable(0);
export const notificationsHasMore = writable(false);
export const notificationsUnreadOnly = writable(false);
export const notificationsLoading = writable(false);
export const notificationsError = writable("");
export const notificationsFeed = derived(
  [workspaceEventNotificationsStore, notificationsStore],
  ([$workspaceEvents, $backendNotifications]): NotificationsFeedItem[] => [
    ...$workspaceEvents.map((notification) => ({ ...notification, source: "workspace_event" as const })),
    ...$backendNotifications.map((notification) => ({ ...notification, source: "backend" as const })),
  ],
);
export const notificationsUnreadCountTotal = derived(
  [notificationsUnreadCount, workspaceEventNotificationsStore],
  ([$backendUnreadCount, $workspaceEvents]) => $backendUnreadCount + $workspaceEvents.length,
);

let currentPageSize = DEFAULT_PAGE_SIZE;
let currentUnreadOnly = false;

function notificationKey(notification: Pick<
  WorkspaceNotification,
  "kind" | "title" | "body" | "route" | "entity_name" | "resource_id" | "tenant_id"
>) {
  return [
    notification.kind,
    notification.title,
    notification.body,
    notification.route,
    notification.entity_name ?? "",
    notification.resource_id ?? "",
    notification.tenant_id ?? "",
  ].join("\u001f");
}

function notificationsMatch(
  a: WorkspaceNotification,
  b: WorkspaceNotification,
  windowMs = WORKSPACE_EVENT_DEDUP_WINDOW_MS,
) {
  if (notificationKey(a) !== notificationKey(b)) return false;

  const aTime = Date.parse(a.created_at);
  const bTime = Date.parse(b.created_at);
  if (Number.isNaN(aTime) || Number.isNaN(bTime)) return true;

  return Math.abs(aTime - bTime) <= windowMs;
}

function workspaceNotificationFromEvent(event: WorkspaceEvent): WorkspaceNotification | null {
  const entityName = event.app_name ?? event.resource_id ?? null;

  switch (event.kind) {
    case "app_created":
      return {
        id: crypto.randomUUID(),
        user_id: event.user_id ?? "",
        tenant_id: event.tenant_id,
        kind: "app_created",
        title: "Application created",
        body: `Application ${entityName ?? "application"} was created.`,
        route: event.app_name ? `/apps/${event.app_name}` : "/apps",
        entity_name: entityName,
        resource_id: event.resource_id,
        metadata: { ...event },
        created_at: new Date().toISOString(),
        read_at: null,
        is_read: false,
      };
    case "app_updated":
      return {
        id: crypto.randomUUID(),
        user_id: event.user_id ?? "",
        tenant_id: event.tenant_id,
        kind: "app_updated",
        title: "Application updated",
        body: `Application ${entityName ?? "application"} was updated.`,
        route: event.app_name ? `/apps/${event.app_name}` : "/apps",
        entity_name: entityName,
        resource_id: event.resource_id,
        metadata: { ...event },
        created_at: new Date().toISOString(),
        read_at: null,
        is_read: false,
      };
    case "app_deleted":
      return {
        id: crypto.randomUUID(),
        user_id: event.user_id ?? "",
        tenant_id: event.tenant_id,
        kind: "app_deleted",
        title: "Application deleted",
        body: `Application ${entityName ?? "application"} was deleted.`,
        route: "/apps",
        entity_name: entityName,
        resource_id: event.resource_id,
        metadata: { ...event },
        created_at: new Date().toISOString(),
        read_at: null,
        is_read: false,
      };
    case "deployment_changed":
      return {
        id: crypto.randomUUID(),
        user_id: event.user_id ?? "",
        tenant_id: event.tenant_id,
        kind: "deployment_changed",
        title: "Deployment changed",
        body: `Deployment activity was recorded for ${entityName ?? "deployment"}.`,
        route: event.app_name ? `/apps/${event.app_name}` : "/apps",
        entity_name: entityName,
        resource_id: event.resource_id,
        metadata: { ...event },
        created_at: new Date().toISOString(),
        read_at: null,
        is_read: false,
      };
    case "profile_updated":
      return {
        id: crypto.randomUUID(),
        user_id: event.user_id ?? "",
        tenant_id: event.tenant_id,
        kind: "profile_updated",
        title: "Profile updated",
        body: "Your profile was updated.",
        route: "/settings",
        entity_name: entityName,
        resource_id: event.resource_id,
        metadata: { ...event },
        created_at: new Date().toISOString(),
        read_at: null,
        is_read: false,
      };
    case "github_accounts_changed":
      return {
        id: crypto.randomUUID(),
        user_id: event.user_id ?? "",
        tenant_id: event.tenant_id,
        kind: "github_accounts_changed",
        title: "GitHub connected",
        body: "Your GitHub integrations changed.",
        route: "/settings",
        entity_name: entityName,
        resource_id: event.resource_id,
        metadata: { ...event },
        created_at: new Date().toISOString(),
        read_at: null,
        is_read: false,
      };
    case "billing_updated":
      return {
        id: crypto.randomUUID(),
        user_id: event.user_id ?? "",
        tenant_id: event.tenant_id,
        kind: "billing_updated",
        title: "Billing updated",
        body: "Your billing status changed.",
        route: "/settings",
        entity_name: entityName,
        resource_id: event.resource_id,
        metadata: { ...event },
        created_at: new Date().toISOString(),
        read_at: null,
        is_read: false,
      };
    case "security_rules_changed":
      return {
        id: crypto.randomUUID(),
        user_id: event.user_id ?? "",
        tenant_id: event.tenant_id,
        kind: "security_rules_changed",
        title: "Security rules changed",
        body: `Security rules were updated for ${entityName ?? "application"}.`,
        route: event.app_name ? `/apps/${event.app_name}` : "/apps",
        entity_name: entityName,
        resource_id: event.resource_id,
        metadata: { ...event },
        created_at: new Date().toISOString(),
        read_at: null,
        is_read: false,
      };
    case "volume_changed":
      return {
        id: crypto.randomUUID(),
        user_id: event.user_id ?? "",
        tenant_id: event.tenant_id,
        kind: "volume_changed",
        title: "Storage updated",
        body: "A storage volume changed.",
        route: "/storage",
        entity_name: entityName,
        resource_id: event.resource_id,
        metadata: { ...event },
        created_at: new Date().toISOString(),
        read_at: null,
        is_read: false,
      };
    case "snapshot_changed":
      return {
        id: crypto.randomUUID(),
        user_id: event.user_id ?? "",
        tenant_id: event.tenant_id,
        kind: "snapshot_changed",
        title: "Snapshot updated",
        body: "A snapshot changed.",
        route: "/storage",
        entity_name: entityName,
        resource_id: event.resource_id,
        metadata: { ...event },
        created_at: new Date().toISOString(),
        read_at: null,
        is_read: false,
      };
    case "database_created":
      return {
        id: crypto.randomUUID(),
        user_id: event.user_id ?? "",
        tenant_id: event.tenant_id,
        kind: "database_created",
        title: "Database created",
        body: `Database ${entityName ?? "database"} was created.`,
        route: event.resource_id ? `/databases/${event.resource_id}` : "/databases",
        entity_name: entityName,
        resource_id: event.resource_id,
        metadata: { ...event },
        created_at: new Date().toISOString(),
        read_at: null,
        is_read: false,
      };
    case "database_updated":
      return {
        id: crypto.randomUUID(),
        user_id: event.user_id ?? "",
        tenant_id: event.tenant_id,
        kind: "database_updated",
        title: "Database updated",
        body: `Database ${entityName ?? "database"} was updated.`,
        route: event.resource_id ? `/databases/${event.resource_id}` : "/databases",
        entity_name: entityName,
        resource_id: event.resource_id,
        metadata: { ...event },
        created_at: new Date().toISOString(),
        read_at: null,
        is_read: false,
      };
    case "database_deleted":
      return {
        id: crypto.randomUUID(),
        user_id: event.user_id ?? "",
        tenant_id: event.tenant_id,
        kind: "database_deleted",
        title: "Database deleted",
        body: `Database ${entityName ?? "database"} was deleted.`,
        route: "/databases",
        entity_name: entityName,
        resource_id: event.resource_id,
        metadata: { ...event },
        created_at: new Date().toISOString(),
        read_at: null,
        is_read: false,
      };
    case "refresh":
      return null;
    default:
      return null;
  }
}

export function recordWorkspaceEventNotification(event: WorkspaceEvent) {
  const notification = workspaceNotificationFromEvent(event);
  if (!notification) return;

  workspaceEventNotificationsStore.update((current) => {
    if (current.some((entry) => notificationsMatch(entry, notification))) {
      return current;
    }

    return [notification, ...current].slice(0, 25);
  });
}

export function dismissWorkspaceEventNotification(notificationId: string) {
  workspaceEventNotificationsStore.update((current) => current.filter((notification) => notification.id !== notificationId));
}

export function clearWorkspaceEventNotifications() {
  workspaceEventNotificationsStore.set([]);
}

function syncWorkspaceEventNotifications(backendNotifications: WorkspaceNotification[]) {
  workspaceEventNotificationsStore.update((current) =>
    current.filter(
      (localNotification) =>
        !backendNotifications.some((backendNotification) => notificationsMatch(localNotification, backendNotification)),
    ),
  );
}

export function clearNotifications() {
  notificationsStore.set([]);
  clearWorkspaceEventNotifications();
  notificationsUnreadCount.set(0);
  notificationsHasMore.set(false);
  notificationsUnreadOnly.set(false);
  notificationsLoading.set(false);
  notificationsError.set("");
  currentPageSize = DEFAULT_PAGE_SIZE;
  currentUnreadOnly = false;
}

export async function refreshNotifications(options: { unreadOnly?: boolean; reset?: boolean } = {}) {
  const token = getToken();
  if (!token) {
    clearNotifications();
    return;
  }

  const nextUnreadOnly = options.unreadOnly ?? currentUnreadOnly;
  const shouldReset = options.reset ?? nextUnreadOnly !== currentUnreadOnly;
  currentUnreadOnly = nextUnreadOnly;
  notificationsUnreadOnly.set(currentUnreadOnly);
  if (shouldReset) {
    currentPageSize = DEFAULT_PAGE_SIZE;
  }

  notificationsLoading.set(true);
  try {
    const result = await getNotifications(token, {
      limit: currentPageSize,
      offset: 0,
      unreadOnly: currentUnreadOnly,
    });
    if (result.error) {
      notificationsError.set(result.error);
      return;
    }

    const notifications = result.data?.notifications ?? [];
    notificationsStore.set(notifications);
    notificationsUnreadCount.set(result.data?.unread_count ?? 0);
    notificationsHasMore.set(result.data?.has_more ?? false);
    notificationsError.set("");
    syncWorkspaceEventNotifications(notifications);
  } catch (error) {
    notificationsError.set(error instanceof Error ? error.message : "Failed to fetch notifications");
  } finally {
    notificationsLoading.set(false);
  }
}

export async function loadMoreNotifications() {
  const token = getToken();
  if (!token || !get(notificationsHasMore)) return;

  notificationsLoading.set(true);
  try {
    const offset = get(notificationsStore).length;
    const result = await getNotifications(token, {
      limit: DEFAULT_PAGE_SIZE,
      offset,
      unreadOnly: currentUnreadOnly,
    });

    if (result.error) {
      notificationsError.set(result.error);
      return;
    }

    const notifications = result.data?.notifications ?? [];
    notificationsStore.update((current) => [...current, ...notifications]);
    currentPageSize = get(notificationsStore).length;
    notificationsUnreadCount.set(result.data?.unread_count ?? 0);
    notificationsHasMore.set(result.data?.has_more ?? false);
    notificationsError.set("");
    syncWorkspaceEventNotifications([...get(notificationsStore)]);
  } catch (error) {
    notificationsError.set(error instanceof Error ? error.message : "Failed to fetch notifications");
  } finally {
    notificationsLoading.set(false);
  }
}

export async function readNotificationById(notificationId: string) {
  const token = getToken();
  if (!token) return;

  const result = await markNotificationRead(token, notificationId);
  if (!result.success) {
    notificationsError.set(result.error ?? "Failed to mark notification as read");
    return;
  }

  await refreshNotifications();
}

export async function readAllNotifications() {
  const token = getToken();
  if (!token) return;

  const result = await markAllNotificationsRead(token);
  if (!result.success) {
    notificationsError.set(result.error ?? "Failed to mark notifications as read");
    return;
  }

  clearWorkspaceEventNotifications();
  await refreshNotifications();
}

export const notifications = {
  subscribe: notificationsStore.subscribe,
};

export function useNotificationsBootstrap() {
  onMount(() => {
    void refreshNotifications();

    const handleAuthChange = () => {
      void refreshNotifications();
    };

    const handleProjectChange = () => {
      void refreshNotifications();
    };

    window.addEventListener("mikrom-auth-change", handleAuthChange);
    window.addEventListener("mikrom-project-change", handleProjectChange);

    return () => {
      window.removeEventListener("mikrom-auth-change", handleAuthChange);
      window.removeEventListener("mikrom-project-change", handleProjectChange);
    };
  });
}
