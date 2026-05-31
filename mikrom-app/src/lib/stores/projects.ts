import { browser } from "$app/environment";
import { onMount } from "svelte";
import { derived, get, writable } from "svelte/store";
import { listProjects, type ProjectInfo } from "$lib/api";
import { getToken } from "$lib/auth";

const ACTIVE_PROJECT_COOKIE = "mikrom_active_project";

function readCookie(name: string) {
  if (!browser) return null;

  const entry = document.cookie
    .split("; ")
    .find((row) => row.startsWith(`${name}=`));

  return entry ? decodeURIComponent(entry.split("=").slice(1).join("=")) : null;
}

function writeCookie(name: string, value: string | null) {
  if (!browser) return;

  if (!value) {
    document.cookie = `${name}=; path=/; max-age=0`;
    return;
  }

  document.cookie = `${name}=${encodeURIComponent(value)}; path=/; max-age=${60 * 60 * 24 * 30}; samesite=lax`;
}

export const projectsStore = writable<ProjectInfo[]>([]);
export const projectsLoading = writable(false);
export const projectsError = writable("");
export const activeProjectSlugStore = writable<string | null>(null);
export const projectSwitchingStore = writable(false);

export const activeProjectStore = derived(
  [projectsStore, activeProjectSlugStore],
  ([$projects, $activeProjectSlug]) =>
    $projects.find((project) => project.tenant_id === $activeProjectSlug) ?? null
);

function emitProjectChange(slug: string | null) {
  if (!browser) return;

  window.dispatchEvent(
    new CustomEvent("mikrom-project-change", {
      detail: { slug },
    })
  );
}

export function setActiveProjectSlug(slug: string | null) {
  activeProjectSlugStore.set(slug);
  writeCookie(ACTIVE_PROJECT_COOKIE, slug);
  emitProjectChange(slug);
}

export function beginProjectSwitch() {
  projectSwitchingStore.set(true);
}

export function endProjectSwitch() {
  projectSwitchingStore.set(false);
}

export async function refreshProjects() {
  const token = getToken();
  if (!token) {
    projectsStore.set([]);
    projectsError.set("");
    projectsLoading.set(false);
    return;
  }

  projectsLoading.set(true);
  try {
    const result = await listProjects(token);
    if (result.error) {
      projectsError.set(result.error);
      return;
    }

    const projects = result.data ?? [];
    projectsStore.set(projects);
    projectsError.set("");

    const currentSlug = get(activeProjectSlugStore);
    const currentIsValid = currentSlug ? projects.some((project) => project.tenant_id === currentSlug) : false;
    const nextSlug = currentIsValid ? currentSlug : projects[0]?.tenant_id ?? null;

    if (nextSlug !== currentSlug) {
      setActiveProjectSlug(nextSlug);
    }
  } catch (error) {
    projectsError.set(error instanceof Error ? error.message : "Failed to fetch projects");
  } finally {
    projectsLoading.set(false);
  }
}

export function useProjectBootstrap() {
  onMount(() => {
    activeProjectSlugStore.set(readCookie(ACTIVE_PROJECT_COOKIE));
    void refreshProjects();

    const handleAuthChange = () => {
      void refreshProjects();
    };

    window.addEventListener("mikrom-auth-change", handleAuthChange);

    return () => {
      window.removeEventListener("mikrom-auth-change", handleAuthChange);
    };
  });
}
