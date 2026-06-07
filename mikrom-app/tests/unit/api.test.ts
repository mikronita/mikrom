import { beforeEach, describe, expect, it, vi } from "vitest";
import { waitFor } from "@testing-library/svelte";

vi.mock("$env/dynamic/public", () => ({
  env: {},
}));

import {
  listNotifications,
  watchAppLogsSSE,
  watchAppMetricsSSE,
  watchDeploymentsSSE,
  watchMeshStatusSSE,
  watchWorkspaceEventsSSE,
} from "$lib/api";

const encoder = new TextEncoder();

beforeEach(() => {
  vi.restoreAllMocks();
});

describe("api SSE helpers", () => {
  it("opens workspace events with authorization headers and no token query", async () => {
    const chunks = [
      encoder.encode('data: {"kind":"refresh","tenant_id":"tenant-1"}\n'),
      encoder.encode("\n"),
    ];

    const read = vi
      .fn()
      .mockResolvedValueOnce({ value: chunks[0], done: false })
      .mockResolvedValueOnce({ value: chunks[1], done: false })
      .mockResolvedValueOnce({ value: undefined, done: true });

    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      body: {
        getReader: () => ({ read }),
      },
    });

    vi.stubGlobal("fetch", fetchMock);

    const onMessage = vi.fn();
    const cleanup = watchWorkspaceEventsSSE("secret-token", onMessage);

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledTimes(1);
    });

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/workspace/events",
      expect.objectContaining({
        headers: {
          Authorization: "Bearer secret-token",
        },
      }),
    );

    await waitFor(() => {
      expect(onMessage).toHaveBeenCalledWith({
        kind: "refresh",
        tenant_id: "tenant-1",
      });
    });

    cleanup();
  });

  it.each([
    ["deployments", (handler: (payload: unknown) => void) => watchDeploymentsSSE("secret-token", handler), "/api/v1/deployments/events"],
    [
      "app metrics",
      (handler: (payload: unknown) => void) => watchAppMetricsSSE("secret-token", "starter", handler),
      "/api/v1/apps/starter/metrics/stream",
    ],
    ["mesh status", (handler: (payload: unknown) => void) => watchMeshStatusSSE("secret-token", handler), "/api/v1/networking/mesh/stream"],
    [
      "app logs",
      (handler: (payload: unknown) => void) => watchAppLogsSSE("secret-token", "starter", handler),
      "/api/v1/apps/starter/logs/stream",
    ],
  ] as const)("opens %s SSE with authorization headers", async (_label, openStream, expectedUrl) => {
    const read = vi
      .fn()
      .mockResolvedValueOnce({ value: encoder.encode('data: {"ok":true}\n\n'), done: false })
      .mockResolvedValueOnce({ value: undefined, done: true });

    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      body: {
        getReader: () => ({ read }),
      },
    });

    vi.stubGlobal("fetch", fetchMock);

    const onMessage = vi.fn();
    const cleanup = openStream(onMessage);

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledTimes(1);
    });

    expect(fetchMock).toHaveBeenCalledWith(
      expectedUrl,
      expect.objectContaining({
        headers: {
          Authorization: "Bearer secret-token",
        },
      }),
    );

    await waitFor(() => {
      expect(onMessage).toHaveBeenCalledWith({ ok: true });
    });

    cleanup();
  });
});

describe("api notifications helper", () => {
  it("passes pagination and unread filter query params", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        notifications: [],
        unread_count: 0,
        has_more: false,
        next_offset: 0,
      }),
    });

    vi.stubGlobal("fetch", fetchMock);

    await listNotifications("secret-token", { limit: 5, offset: 10, unreadOnly: true });

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/notifications?limit=5&offset=10&unread_only=true",
      expect.objectContaining({
        headers: {
          "Content-Type": "application/json",
          Authorization: "Bearer secret-token",
        },
      }),
    );
  });
});
