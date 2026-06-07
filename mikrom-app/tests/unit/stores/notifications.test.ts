import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import {
  clearNotifications,
  loadMoreNotifications,
  notificationsError,
  notificationsHasMore,
  notificationsLoading,
  notificationsStore,
  notificationsUnreadCount,
  notificationsUnreadOnly,
  readAllNotifications,
  readNotificationById,
  refreshNotifications,
} from "$lib/stores/notifications";
import { getToken } from "$lib/auth";
import { getNotifications, markAllNotificationsRead, markNotificationRead } from "$lib/api";

vi.mock("$lib/auth", () => ({
  getToken: vi.fn(),
}));

vi.mock("$lib/api", () => ({
  getNotifications: vi.fn(),
  markNotificationRead: vi.fn(),
  markAllNotificationsRead: vi.fn(),
}));

const mockedGetToken = vi.mocked(getToken);
const mockedGetNotifications = vi.mocked(getNotifications);
const mockedMarkNotificationRead = vi.mocked(markNotificationRead);
const mockedMarkAllNotificationsRead = vi.mocked(markAllNotificationsRead);

const sampleNotification = {
  id: "notification-1",
  user_id: "user-1",
  tenant_id: null,
  kind: "app_created",
  title: "Application created",
  body: "Application starter was created.",
  route: "/apps/starter",
  entity_name: "starter",
  resource_id: null,
  metadata: {},
  created_at: "2026-06-01T12:00:00.000Z",
  read_at: null,
  is_read: false,
};

beforeEach(() => {
  clearNotifications();
  mockedGetToken.mockReset();
  mockedGetNotifications.mockReset();
  mockedMarkNotificationRead.mockReset();
  mockedMarkAllNotificationsRead.mockReset();
});

describe("notifications store", () => {
  it("hydrates notifications and unread count on refresh", async () => {
    mockedGetToken.mockReturnValue("token");
    mockedGetNotifications.mockResolvedValue({
      data: {
        notifications: [sampleNotification],
        unread_count: 1,
        has_more: false,
        next_offset: 1,
      },
    });

    await refreshNotifications();

    expect(mockedGetNotifications).toHaveBeenCalledWith("token", {
      limit: 10,
      offset: 0,
      unreadOnly: false,
    });
    expect(get(notificationsStore)).toEqual([sampleNotification]);
    expect(get(notificationsUnreadCount)).toBe(1);
    expect(get(notificationsHasMore)).toBe(false);
    expect(get(notificationsError)).toBe("");
    expect(get(notificationsLoading)).toBe(false);
  });

  it("loads more notifications using the current filter and appends results", async () => {
    mockedGetToken.mockReturnValue("token");
    mockedGetNotifications
      .mockResolvedValueOnce({
        data: {
          notifications: [sampleNotification],
          unread_count: 1,
          has_more: true,
          next_offset: 1,
        },
      })
      .mockResolvedValueOnce({
        data: {
          notifications: [
            {
              ...sampleNotification,
              id: "notification-2",
              created_at: "2026-06-01T11:55:00.000Z",
            },
          ],
          unread_count: 1,
          has_more: false,
          next_offset: 2,
        },
      });

    await refreshNotifications();
    await loadMoreNotifications();

    expect(mockedGetNotifications).toHaveBeenNthCalledWith(1, "token", {
      limit: 10,
      offset: 0,
      unreadOnly: false,
    });
    expect(mockedGetNotifications).toHaveBeenNthCalledWith(2, "token", {
      limit: 10,
      offset: 1,
      unreadOnly: false,
    });
    expect(get(notificationsStore)).toHaveLength(2);
    expect(get(notificationsHasMore)).toBe(false);
  });

  it("refreshes only unread notifications when requested", async () => {
    mockedGetToken.mockReturnValue("token");
    mockedGetNotifications.mockResolvedValue({
      data: {
        notifications: [sampleNotification],
        unread_count: 1,
        has_more: false,
        next_offset: 1,
      },
    });

    await refreshNotifications({ unreadOnly: true, reset: true });

    expect(mockedGetNotifications).toHaveBeenCalledWith("token", {
      limit: 10,
      offset: 0,
      unreadOnly: true,
    });
    expect(get(notificationsUnreadOnly)).toBe(true);
  });

  it("marks a notification as read and refreshes the list", async () => {
    mockedGetToken.mockReturnValue("token");
    mockedMarkNotificationRead.mockResolvedValue({ success: true });
    mockedGetNotifications.mockResolvedValue({
      data: {
        notifications: [{ ...sampleNotification, is_read: true, read_at: "2026-06-01T12:01:00.000Z" }],
        unread_count: 0,
        has_more: false,
        next_offset: 1,
      },
    });

    await readNotificationById("notification-1");

    expect(mockedMarkNotificationRead).toHaveBeenCalledWith("token", "notification-1");
    expect(get(notificationsUnreadCount)).toBe(0);
    expect(get(notificationsStore)[0].is_read).toBe(true);
  });

  it("marks all notifications as read", async () => {
    mockedGetToken.mockReturnValue("token");
    mockedMarkAllNotificationsRead.mockResolvedValue({ success: true });
    mockedGetNotifications.mockResolvedValue({
      data: { notifications: [], unread_count: 0, has_more: false, next_offset: 0 },
    });

    await readAllNotifications();

    expect(mockedMarkAllNotificationsRead).toHaveBeenCalledWith("token");
    expect(get(notificationsUnreadCount)).toBe(0);
  });
});
