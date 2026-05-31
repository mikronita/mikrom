import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import {
  activeProjectSlugStore,
  projectsError,
  projectsLoading,
  projectsStore,
  refreshProjects,
  setActiveProjectSlug,
} from "$lib/stores/projects";
import { getToken } from "$lib/auth";
import { listProjects } from "$lib/api";

vi.mock("$lib/auth", () => ({
  getToken: vi.fn(),
}));

vi.mock("$lib/api", () => ({
  listProjects: vi.fn(),
}));

const mockedGetToken = vi.mocked(getToken);
const mockedListProjects = vi.mocked(listProjects);

const projects = [
  {
    id: "project-1",
    tenant_id: "acme",
    name: "Acme",
    created_at: "2026-05-01T10:00:00.000Z",
  },
  {
    id: "project-2",
    tenant_id: "beta",
    name: "Beta",
    created_at: "2026-05-02T10:00:00.000Z",
  },
];

beforeEach(() => {
  projectsStore.set([]);
  projectsError.set("");
  projectsLoading.set(false);
  activeProjectSlugStore.set(null);
  mockedGetToken.mockReset();
  mockedListProjects.mockReset();
});

describe("projects store", () => {
  it("selects the first available project when the current one is invalid", async () => {
    setActiveProjectSlug("stale");
    mockedGetToken.mockReturnValue("token");
    mockedListProjects.mockResolvedValue({ data: projects });

    await refreshProjects();

    expect(mockedListProjects).toHaveBeenCalledWith("token");
    expect(get(projectsStore)).toEqual(projects);
    expect(get(activeProjectSlugStore)).toBe("acme");
    expect(document.cookie).toContain("mikrom_active_project=acme");
    expect(get(projectsError)).toBe("");
    expect(get(projectsLoading)).toBe(false);
  });

  it("keeps loading state and surfaces backend failures", async () => {
    mockedGetToken.mockReturnValue("token");
    mockedListProjects.mockResolvedValue({ error: "project service unavailable" });

    await refreshProjects();

    expect(get(projectsError)).toBe("project service unavailable");
    expect(get(projectsLoading)).toBe(false);
  });

  it("persists the active project in cookie and localStorage", () => {
    setActiveProjectSlug("beta");

    expect(get(activeProjectSlugStore)).toBe("beta");
    expect(document.cookie).toContain("mikrom_active_project=beta");
    expect(localStorage.getItem("mikrom_active_project")).toBe("beta");
  });

  it("clears the persisted active project when reset", () => {
    setActiveProjectSlug("beta");
    setActiveProjectSlug(null);

    expect(get(activeProjectSlugStore)).toBeNull();
    expect(document.cookie).toContain("mikrom_active_project=");
    expect(localStorage.getItem("mikrom_active_project")).toBeNull();
  });
});
