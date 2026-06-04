import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { waitFor } from "@testing-library/svelte";

const mocks = vi.hoisted(() => ({
  getToken: vi.fn(),
  listDatabases: vi.fn(),
  createDatabase: vi.fn(),
  deleteDatabase: vi.fn(),
  getDatabaseConnection: vi.fn(),
}));

vi.mock("$lib/auth", () => ({
  getToken: mocks.getToken,
}));

vi.mock("$lib/api", () => ({
  listDatabases: mocks.listDatabases,
  createDatabase: mocks.createDatabase,
  deleteDatabase: mocks.deleteDatabase,
  getDatabaseConnection: mocks.getDatabaseConnection,
}));

import {
  clearDatabases,
  createDatabase,
  databasesError,
  databasesLoading,
  databasesStore,
  deleteDatabase,
  refreshDatabases,
} from "$lib/stores/databases";

beforeEach(() => {
  clearDatabases();
  mocks.getToken.mockReset();
  mocks.listDatabases.mockReset();
  mocks.createDatabase.mockReset();
  mocks.deleteDatabase.mockReset();
  mocks.getDatabaseConnection.mockReset();
});

describe("databases store", () => {
  it("maps backend statuses and sorts by creation date", async () => {
    mocks.getToken.mockReturnValue("token");
    mocks.listDatabases.mockResolvedValue({
      data: [
        {
          id: "db-1",
          name: "archive-db",
          engine: "neon",
          postgres_version: 15,
          status: "pending",
          vcpus: 1,
          memory_mib: 512,
          disk_mib: 10240,
          created_at: "2026-05-01T10:00:00.000Z",
          updated_at: "2026-05-01T10:00:00.000Z",
        },
        {
          id: "db-2",
          name: "orders-db",
          engine: "neon",
          postgres_version: 16,
          status: "running",
          vcpus: 2,
          memory_mib: 4096,
          disk_mib: 20480,
          created_at: "2026-05-02T10:00:00.000Z",
          updated_at: "2026-05-02T10:00:00.000Z",
        },
      ],
    });

    await refreshDatabases();

    await waitFor(() => {
      expect(mocks.listDatabases).toHaveBeenCalledWith("token");
    });

    expect(get(databasesStore)).toEqual([
      expect.objectContaining({ id: "db-2", status: "Running", postgres_version: 16 }),
      expect.objectContaining({ id: "db-1", status: "Provisioning", postgres_version: 15 }),
    ]);
    expect(get(databasesLoading)).toBe(false);
    expect(get(databasesError)).toBe("");
  });

  it("creates and removes databases through the API", async () => {
    mocks.getToken.mockReturnValue("token");
    mocks.createDatabase.mockResolvedValue({
      data: {
        id: "db-3",
        name: "analytics",
        engine: "neon",
        postgres_version: 16,
        status: "pending",
        vcpus: 4,
        memory_mib: 8192,
        disk_mib: 25600,
        created_at: "2026-05-03T10:00:00.000Z",
        updated_at: "2026-05-03T10:00:00.000Z",
      },
    });
    mocks.deleteDatabase.mockResolvedValue({ success: true });

    await createDatabase({
      name: "analytics",
      engine: "neon",
      postgres_version: 16,
      vcpus: 4,
      memory_mib: 8192,
      disk_mib: 25600,
    });

    expect(mocks.createDatabase).toHaveBeenCalledWith("token", {
      name: "analytics",
      engine: "neon",
      postgres_version: 16,
      vcpus: 4,
      memory_mib: 8192,
      disk_mib: 25600,
    });
    expect(get(databasesStore)[0]).toMatchObject({
      id: "db-3",
      name: "analytics",
      status: "Provisioning",
    });

    await deleteDatabase("db-3");

    expect(mocks.deleteDatabase).toHaveBeenCalledWith("token", "db-3");
    expect(get(databasesStore)).toEqual([]);
  });
});
