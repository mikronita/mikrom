import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import NotificationsMenu from "$lib/components/NotificationsMenu.svelte";

type NotificationItem = {
  id: string;
  user_id: string;
  tenant_id: string | null;
  kind: string;
  title: string;
  body: string;
  route: string;
  entity_name: string | null;
  resource_id: string | null;
  metadata: Record<string, unknown>;
  created_at: string;
  read_at: string | null;
  is_read: boolean;
};

const mocks = vi.hoisted(() => {
  const initialNotifications: NotificationItem[] = [
    {
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
    },
  ];

  const createWritableStore = <T>(initialValue: T) => {
    let value = initialValue;
    const subscribers = new Set<(value: T) => void>();
    return {
      subscribe(run: (value: T) => void) {
        run(value);
        subscribers.add(run);
        return () => subscribers.delete(run);
      },
      set(next: T) {
        value = next;
        subscribers.forEach((run) => run(value));
      },
    };
  };

  return {
    initialNotifications,
    goto: vi.fn(),
    useNotificationsBootstrap: vi.fn(),
    readAllNotifications: vi.fn(),
    readNotificationById: vi.fn(),
    refreshNotifications: vi.fn(),
    loadMoreNotifications: vi.fn(),
    notificationsStore: createWritableStore([...initialNotifications]),
    notificationsUnreadCount: createWritableStore(1),
    notificationsHasMore: createWritableStore(false),
    notificationsUnreadOnly: createWritableStore(false),
    notificationsLoading: createWritableStore(false),
    notificationsError: createWritableStore(""),
  };
});

vi.mock("$app/navigation", () => ({
  goto: mocks.goto,
}));

vi.mock("$lib/stores/notifications", () => ({
  notificationsStore: mocks.notificationsStore,
  notificationsUnreadCount: mocks.notificationsUnreadCount,
  notificationsHasMore: mocks.notificationsHasMore,
  notificationsUnreadOnly: mocks.notificationsUnreadOnly,
  notificationsLoading: mocks.notificationsLoading,
  notificationsError: mocks.notificationsError,
  readAllNotifications: mocks.readAllNotifications,
  readNotificationById: mocks.readNotificationById,
  refreshNotifications: mocks.refreshNotifications,
  loadMoreNotifications: mocks.loadMoreNotifications,
  useNotificationsBootstrap: mocks.useNotificationsBootstrap,
}));

beforeEach(() => {
  mocks.goto.mockReset();
  mocks.useNotificationsBootstrap.mockReset();
  mocks.readAllNotifications.mockReset();
  mocks.readNotificationById.mockReset();
  mocks.refreshNotifications.mockReset();
  mocks.loadMoreNotifications.mockReset();
  mocks.notificationsUnreadCount.set(1);
  mocks.notificationsHasMore.set(false);
  mocks.notificationsUnreadOnly.set(false);
  mocks.notificationsLoading.set(false);
  mocks.notificationsError.set("");
  mocks.notificationsStore.set([...mocks.initialNotifications]);
});

describe("NotificationsMenu", () => {
  it("shows the unread badge and notification list", async () => {
    const { unmount } = render(NotificationsMenu);

    expect(screen.getByLabelText("Notifications")).toBeTruthy();
    expect(screen.getByText("1")).toBeTruthy();

    await fireEvent.click(screen.getByLabelText("Notifications"));

    await waitFor(() => {
      expect(screen.getByText("Mark all read")).toBeTruthy();
      expect(screen.getByText("Unread only")).toBeTruthy();
      expect(screen.getByText("Application created")).toBeTruthy();
      expect(screen.getByText("Application starter was created.")).toBeTruthy();
    });

    await fireEvent.click(screen.getByLabelText("Notifications"));
    unmount();
  });

  it("allows switching to unread only and loading more", async () => {
    mocks.notificationsHasMore.set(true);

    const { unmount } = render(NotificationsMenu);

    await fireEvent.click(screen.getByLabelText("Notifications"));
    await waitFor(() => {
      expect(screen.getByText("Unread only")).toBeTruthy();
      expect(screen.getByText("Load more")).toBeTruthy();
    });

    await fireEvent.click(screen.getByText("Unread only"));

    expect(mocks.refreshNotifications).toHaveBeenCalledWith({ unreadOnly: true, reset: true });

    await fireEvent.click(screen.getByText("Load more"));

    expect(mocks.loadMoreNotifications).toHaveBeenCalled();

    await fireEvent.click(screen.getByLabelText("Notifications"));
    unmount();
  });
});
