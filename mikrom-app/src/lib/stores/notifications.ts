import { onMount } from "svelte";
import { get } from "svelte/store";
import { writable } from "svelte/store";
import {
  getNotifications,
  markAllNotificationsRead,
  markNotificationRead,
  type WorkspaceNotification,
} from "$lib/api";
import { getToken } from "$lib/auth";

const DEFAULT_PAGE_SIZE = 10;

export const notificationsStore = writable<WorkspaceNotification[]>([]);
export const notificationsUnreadCount = writable(0);
export const notificationsHasMore = writable(false);
export const notificationsUnreadOnly = writable(false);
export const notificationsLoading = writable(false);
export const notificationsError = writable("");

let currentPageSize = DEFAULT_PAGE_SIZE;
let currentUnreadOnly = false;

export function clearNotifications() {
  notificationsStore.set([]);
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
