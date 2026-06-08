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

type NotificationFeedItem = NotificationItem & {
  source: "backend" | "workspace_event";
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

  const initialFeed: NotificationFeedItem[] = [
    { ...initialNotifications[0], source: "backend" },
  ];

  const createWritableStore = <T>(initialValue: T) => {
    let value = initialValue;
    const subscribers = new Set<(value: T) => void>();
    return {
      get() {
        return value;
      },
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
    notificationsFeed: createWritableStore([...initialFeed]),
    notificationsUnreadCountTotal: createWritableStore(1),
    notificationsHasMore: createWritableStore(false),
    notificationsUnreadOnly: createWritableStore(false),
    notificationsLoading: createWritableStore(false),
    notificationsError: createWritableStore(""),
    activeProjectSlugStore: createWritableStore<string | null>("alpha"),
    clearWorkspaceEventNotifications: vi.fn(() => {
      const liveItems = mocks.notificationsFeed.get().filter((item) => item.source === "workspace_event");
      if (liveItems.length === 0) return;

      mocks.notificationsFeed.set(
        mocks.notificationsFeed.get().filter((item) => item.source !== "workspace_event"),
      );
      mocks.notificationsUnreadCountTotal.set(0);
    }),
  };
});

const liveUpdatesStateKey = (slug: string | null) =>
  `mikrom_notifications_live_updates_visible:${slug ?? "global"}`;
const unreadOnlyStateKey = (slug: string | null) =>
  `mikrom_notifications_unread_only:${slug ?? "global"}`;
const scrollStateKey = (slug: string | null) =>
  `mikrom_notifications_scroll_top:${slug ?? "global"}`;
const widthStateKey = (slug: string | null) =>
  `mikrom_notifications_width_mode:${slug ?? "global"}`;

vi.mock("$app/navigation", () => ({
  goto: mocks.goto,
}));

vi.mock("$lib/stores/notifications", () => ({
  notificationsFeed: mocks.notificationsFeed,
  notificationsUnreadCountTotal: mocks.notificationsUnreadCountTotal,
  notificationsHasMore: mocks.notificationsHasMore,
  notificationsUnreadOnly: mocks.notificationsUnreadOnly,
  notificationsLoading: mocks.notificationsLoading,
  notificationsError: mocks.notificationsError,
  dismissWorkspaceEventNotification: vi.fn(),
  clearWorkspaceEventNotifications: mocks.clearWorkspaceEventNotifications,
  readAllNotifications: mocks.readAllNotifications,
  readNotificationById: mocks.readNotificationById,
  refreshNotifications: mocks.refreshNotifications,
  loadMoreNotifications: mocks.loadMoreNotifications,
  useNotificationsBootstrap: mocks.useNotificationsBootstrap,
}));

vi.mock("$lib/stores/projects", () => ({
  activeProjectSlugStore: mocks.activeProjectSlugStore,
}));

beforeEach(() => {
  localStorage.removeItem(liveUpdatesStateKey("alpha"));
  localStorage.removeItem(liveUpdatesStateKey("beta"));
  localStorage.removeItem(liveUpdatesStateKey(null));
  localStorage.removeItem(unreadOnlyStateKey("alpha"));
  localStorage.removeItem(unreadOnlyStateKey("beta"));
  localStorage.removeItem(unreadOnlyStateKey(null));
  localStorage.removeItem(scrollStateKey("alpha"));
  localStorage.removeItem(scrollStateKey("beta"));
  localStorage.removeItem(scrollStateKey(null));
  localStorage.removeItem(widthStateKey("alpha"));
  localStorage.removeItem(widthStateKey("beta"));
  localStorage.removeItem(widthStateKey(null));
  mocks.goto.mockReset();
  mocks.useNotificationsBootstrap.mockReset();
  mocks.readAllNotifications.mockReset();
  mocks.readNotificationById.mockReset();
  mocks.refreshNotifications.mockReset();
  mocks.loadMoreNotifications.mockReset();
  mocks.notificationsUnreadCountTotal.set(1);
  mocks.notificationsHasMore.set(false);
  mocks.notificationsUnreadOnly.set(false);
  mocks.notificationsLoading.set(false);
  mocks.notificationsError.set("");
  mocks.activeProjectSlugStore.set("alpha");
  mocks.notificationsFeed.set([
    {
      ...mocks.initialNotifications[0],
      source: "backend",
    },
  ]);
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

  it("shows live workspace events in the feed", async () => {
    mocks.notificationsFeed.set([
      {
        id: "workspace-event-1",
        user_id: "user-1",
        tenant_id: null,
        kind: "billing_updated",
        title: "Billing updated",
        body: "Your billing status changed.",
        route: "/settings",
        entity_name: null,
        resource_id: null,
        metadata: {},
        created_at: "2026-06-01T12:05:00.000Z",
        read_at: null,
        is_read: false,
        source: "workspace_event",
      },
    ]);
    mocks.notificationsUnreadCountTotal.set(1);

    const { unmount } = render(NotificationsMenu);

    await fireEvent.click(screen.getByLabelText("Notifications"));

    await waitFor(() => {
      expect(screen.getByText("Billing updated")).toBeTruthy();
      expect(screen.getByText("Your billing status changed.")).toBeTruthy();
      expect(screen.getByText("Live")).toBeTruthy();
    });

    await fireEvent.click(screen.getByLabelText("Notifications"));
    unmount();
  });

  it("collapses and clears live updates", async () => {
    mocks.notificationsFeed.set([
      {
        id: "workspace-event-1",
        user_id: "user-1",
        tenant_id: null,
        kind: "billing_updated",
        title: "Billing updated",
        body: "Your billing status changed.",
        route: "/settings",
        entity_name: null,
        resource_id: null,
        metadata: {},
        created_at: "2026-06-01T12:05:00.000Z",
        read_at: null,
        is_read: false,
        source: "workspace_event",
      },
    ]);
    mocks.notificationsUnreadCountTotal.set(1);

    const { unmount } = render(NotificationsMenu);

    await fireEvent.click(screen.getByLabelText("Notifications"));

    await waitFor(() => {
      expect(screen.getByText("Live updates")).toBeTruthy();
      expect(screen.getByText("Hide")).toBeTruthy();
      expect(screen.getByText("Clear")).toBeTruthy();
    });

    await fireEvent.click(screen.getByText("Hide"));

    await waitFor(() => {
      expect(screen.getByText("Show")).toBeTruthy();
    });

    await fireEvent.click(screen.getByText("Clear"));

    await waitFor(() => {
      expect(mocks.clearWorkspaceEventNotifications).toHaveBeenCalled();
      expect(screen.queryByText("Live updates")).toBeNull();
    });

    await fireEvent.click(screen.getByLabelText("Notifications"));
    unmount();
  });

  it("remembers the live updates collapsed state between renders", async () => {
    mocks.notificationsFeed.set([
      {
        id: "workspace-event-1",
        user_id: "user-1",
        tenant_id: null,
        kind: "billing_updated",
        title: "Billing updated",
        body: "Your billing status changed.",
        route: "/settings",
        entity_name: null,
        resource_id: null,
        metadata: {},
        created_at: "2026-06-01T12:05:00.000Z",
        read_at: null,
        is_read: false,
        source: "workspace_event",
      },
    ]);
    mocks.notificationsUnreadCountTotal.set(1);

    const { unmount } = render(NotificationsMenu);

    await fireEvent.click(screen.getByLabelText("Notifications"));
    await waitFor(() => {
      expect(screen.getByText("Hide")).toBeTruthy();
    });

    await fireEvent.click(screen.getByText("Hide"));
    await waitFor(() => {
      expect(localStorage.getItem(liveUpdatesStateKey("alpha"))).toBe("false");
      expect(screen.getByText("Show")).toBeTruthy();
    });

    unmount();

    mocks.activeProjectSlugStore.set("beta");
    const rerendered = render(NotificationsMenu);
    await fireEvent.click(screen.getByLabelText("Notifications"));

    await waitFor(() => {
      expect(screen.getByText("Hide")).toBeTruthy();
    });

    rerendered.unmount();

    mocks.activeProjectSlugStore.set("alpha");
    const alphaRerendered = render(NotificationsMenu);
    await fireEvent.click(screen.getByLabelText("Notifications"));

    await waitFor(() => {
      expect(screen.getByText("Show")).toBeTruthy();
      expect(screen.queryByText("Hide")).toBeNull();
    });

    alphaRerendered.unmount();
  });

  it("remembers the unread only filter per project", async () => {
    const { unmount } = render(NotificationsMenu);

    await fireEvent.click(screen.getByLabelText("Notifications"));
    await waitFor(() => {
      expect(screen.getByText("Unread only")).toBeTruthy();
    });

    await fireEvent.click(screen.getByText("Unread only"));

    await waitFor(() => {
      expect(localStorage.getItem(unreadOnlyStateKey("alpha"))).toBe("true");
      expect(mocks.refreshNotifications).toHaveBeenCalledWith({ unreadOnly: true, reset: true });
    });

    unmount();

    mocks.activeProjectSlugStore.set("beta");
    const betaRender = render(NotificationsMenu);
    await fireEvent.click(screen.getByLabelText("Notifications"));

    await waitFor(() => {
      expect(screen.getByText("Unread only").className).not.toContain("bg-background");
      expect(screen.getByText("All").className).toContain("bg-background");
    });

    betaRender.unmount();

    mocks.activeProjectSlugStore.set("alpha");
    const alphaRender = render(NotificationsMenu);
    await fireEvent.click(screen.getByLabelText("Notifications"));

    await waitFor(() => {
      expect(screen.getByText("Unread only").className).toContain("bg-background");
      expect(screen.getByText("All").className).not.toContain("bg-background");
      expect(localStorage.getItem(unreadOnlyStateKey("alpha"))).toBe("true");
    });

    alphaRender.unmount();
  });

  it("remembers the dropdown scroll position per project", async () => {
    localStorage.setItem(scrollStateKey("alpha"), "120");

    const { unmount } = render(NotificationsMenu);
    await fireEvent.click(screen.getByLabelText("Notifications"));

    await waitFor(() => {
      const scrollContainer = screen.getByTestId("notifications-scroll-container");
      expect(scrollContainer.scrollTop).toBe(120);
    });

    const alphaScrollContainer = screen.getByTestId("notifications-scroll-container");
    alphaScrollContainer.scrollTop = 88;
    await fireEvent.scroll(alphaScrollContainer);

    await waitFor(() => {
      expect(localStorage.getItem(scrollStateKey("alpha"))).toBe("88");
    });

    unmount();

    mocks.activeProjectSlugStore.set("beta");
    const betaRender = render(NotificationsMenu);
    await fireEvent.click(screen.getByLabelText("Notifications"));

    await waitFor(() => {
      expect(screen.getByTestId("notifications-scroll-container").scrollTop).toBe(0);
    });

    betaRender.unmount();
  });

  it("remembers the dropdown width per project", async () => {
    localStorage.setItem(widthStateKey("alpha"), "wide");

    const { unmount } = render(NotificationsMenu);
    await fireEvent.click(screen.getByLabelText("Notifications"));

    await waitFor(() => {
      expect(screen.getByTestId("notifications-dropdown").className).toContain("w-[28rem]");
    });

    await fireEvent.click(screen.getByText("Compact"));

    await waitFor(() => {
      expect(localStorage.getItem(widthStateKey("alpha"))).toBe("compact");
      expect(screen.getByTestId("notifications-dropdown").className).toContain("w-[18rem]");
    });

    unmount();

    mocks.activeProjectSlugStore.set("beta");
    const betaRender = render(NotificationsMenu);
    await fireEvent.click(screen.getByLabelText("Notifications"));

    await waitFor(() => {
      expect(screen.getByTestId("notifications-dropdown").className).toContain("w-[22rem]");
    });

    betaRender.unmount();
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
