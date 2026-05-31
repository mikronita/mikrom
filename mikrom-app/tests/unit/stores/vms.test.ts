import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { waitFor } from "@testing-library/svelte";

const mocks = vi.hoisted(() => ({
  getToken: vi.fn(),
  listVms: vi.fn(),
  watchVmsSSE: vi.fn(),
  refreshApps: vi.fn(),
}));

vi.mock("$lib/auth", () => ({
  getToken: mocks.getToken,
}));

vi.mock("$lib/api", () => ({
  listVms: mocks.listVms,
  watchVmsSSE: mocks.watchVmsSSE,
}));

vi.mock("$lib/stores/apps", () => ({
  refreshApps: mocks.refreshApps,
}));

import { clearVms, initVmsSSE, stopVmsSSE, vmsLoading, vmsStore } from "$lib/stores/vms";

const initialVm = {
  job_id: "job-1",
  deployment_id: "deploy-1",
  app_id: "app-1",
  app_name: "starter",
  image: "ghcr.io/mikrom/starter:latest",
  status: "RUNNING",
  host_id: "host-1",
  vm_id: "vm-1",
  cpu_usage: 20,
  ram_used_bytes: 128,
};

let onMessage: ((vm: typeof initialVm) => void) | null = null;

beforeEach(() => {
  clearVms();
  stopVmsSSE();
  onMessage = null;
  mocks.getToken.mockReset().mockReturnValue("token");
  mocks.listVms.mockReset();
  mocks.watchVmsSSE.mockReset();
  mocks.refreshApps.mockReset();
});

describe("vms store", () => {
  it("seeds the store and reacts to SSE updates", async () => {
    mocks.listVms.mockResolvedValue({ data: [initialVm] });
    mocks.watchVmsSSE.mockImplementation((_token: string, handler: (vm: typeof initialVm) => void) => {
      onMessage = handler;
      return vi.fn();
    });

    initVmsSSE();

    await waitFor(() => {
      expect(mocks.listVms).toHaveBeenCalledWith("token");
    });
    await waitFor(() => {
      expect(get(vmsStore)).toEqual([initialVm]);
      expect(get(vmsLoading)).toBe(false);
    });

    onMessage?.({ ...initialVm, status: "STOPPED" });

    expect(get(vmsStore)).toEqual([]);
    await waitFor(() => {
      expect(mocks.refreshApps).toHaveBeenCalled();
    });
  });

  it("does nothing when no token is available", () => {
    mocks.getToken.mockReturnValue(null);

    initVmsSSE();

    expect(mocks.watchVmsSSE).not.toHaveBeenCalled();
    expect(get(vmsStore)).toEqual([]);
  });
});
