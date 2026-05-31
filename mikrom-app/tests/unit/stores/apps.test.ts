import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { appsError, appsLoading, appsStore, refreshApps } from "$lib/stores/apps";
import { getToken } from "$lib/auth";
import { listApps } from "$lib/api";

vi.mock("$lib/auth", () => ({
  getToken: vi.fn(),
}));

vi.mock("$lib/api", () => ({
  listApps: vi.fn(),
}));

const mockedGetToken = vi.mocked(getToken);
const mockedListApps = vi.mocked(listApps);

const sampleApp = {
  id: "app-1",
  name: "starter",
  git_url: "https://github.com/mikrom/starter",
  port: 3000,
  hostname: null,
  active_deployment_id: null,
  desired_replicas: 1,
  min_replicas: 1,
  max_replicas: 1,
  autoscaling_enabled: false,
  cpu_threshold: 80,
  mem_threshold: 80,
  scale_state: "active" as const,
  created_at: "2026-05-01T10:00:00.000Z",
};

beforeEach(() => {
  appsStore.set([]);
  appsError.set("");
  appsLoading.set(false);
  mockedGetToken.mockReset();
  mockedListApps.mockReset();
});

describe("apps store", () => {
  it("hydrates app data on refresh", async () => {
    mockedGetToken.mockReturnValue("token");
    mockedListApps.mockResolvedValue({ data: [sampleApp] });

    await refreshApps();

    expect(mockedListApps).toHaveBeenCalledWith("token");
    expect(get(appsStore)).toEqual([sampleApp]);
    expect(get(appsError)).toBe("");
    expect(get(appsLoading)).toBe(false);
  });

  it("stores API errors without clearing the current data unexpectedly", async () => {
    appsStore.set([sampleApp]);
    mockedGetToken.mockReturnValue("token");
    mockedListApps.mockResolvedValue({ error: "backend unavailable" });

    await refreshApps();

    expect(get(appsStore)).toEqual([sampleApp]);
    expect(get(appsError)).toBe("backend unavailable");
    expect(get(appsLoading)).toBe(false);
  });
});
