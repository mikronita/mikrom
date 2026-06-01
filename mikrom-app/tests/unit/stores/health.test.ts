import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { waitFor } from "@testing-library/svelte";

const mocks = vi.hoisted(() => ({
  health: vi.fn(),
  watchHealthSSE: vi.fn(),
}));

vi.mock("$lib/api", () => ({
  health: mocks.health,
  watchHealthSSE: mocks.watchHealthSSE,
}));

import { healthLoading, healthStore, initHealthStream } from "$lib/stores/health";

let onMessage: ((payload: { status: string; version: string; services?: Record<string, string> }) => void) | null = null;
let cleanup: (() => void) | null = null;

beforeEach(() => {
  healthStore.set(null);
  healthLoading.set(false);
  onMessage = null;
  cleanup = null;
  mocks.health.mockReset();
  mocks.watchHealthSSE.mockReset();
});

afterEach(() => {
  cleanup?.();
});

describe("health store", () => {
  it("hydrates once and then listens to SSE updates", async () => {
    mocks.health.mockResolvedValue({
      status: "ok",
      version: "1.0.0",
      services: { API: "ONLINE" },
    });
    mocks.watchHealthSSE.mockImplementation((handler) => {
      onMessage = handler;
      return vi.fn();
    });

    cleanup = initHealthStream();

    await waitFor(() => {
      expect(mocks.health).toHaveBeenCalledTimes(1);
    });
    await waitFor(() => {
      expect(get(healthStore)).toEqual({
        status: "ok",
        version: "1.0.0",
        services: { API: "ONLINE" },
      });
      expect(get(healthLoading)).toBe(false);
    });

    onMessage?.({
      status: "ok",
      version: "1.0.1",
      services: { API: "ONLINE", Router: "OFFLINE" },
    });

    expect(get(healthStore)).toEqual({
      status: "ok",
      version: "1.0.1",
      services: { API: "ONLINE", Router: "OFFLINE" },
    });

    cleanup?.();
  });

  it("restarts the SSE stream after cleanup", async () => {
    mocks.health.mockResolvedValue({
      status: "ok",
      version: "1.0.0",
      services: {},
    });
    mocks.watchHealthSSE.mockImplementation(() => vi.fn());

    const firstCleanup = initHealthStream();

    await waitFor(() => {
      expect(mocks.health).toHaveBeenCalledTimes(1);
      expect(mocks.watchHealthSSE).toHaveBeenCalledTimes(1);
    });

    firstCleanup();

    const secondCleanup = initHealthStream();

    await waitFor(() => {
      expect(mocks.health).toHaveBeenCalledTimes(2);
      expect(mocks.watchHealthSSE).toHaveBeenCalledTimes(2);
    });

    secondCleanup();
  });
});
