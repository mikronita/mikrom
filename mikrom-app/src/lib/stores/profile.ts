import { browser } from "$app/environment";
import { onMount } from "svelte";
import { writable } from "svelte/store";
import { getToken } from "$lib/auth";
import { getUserProfile, watchWorkspaceEvents, type UserProfile } from "$lib/api";

const PROFILE_CACHE_KEY = "mikrom_profile";

function readCachedProfile(): UserProfile | null {
  if (!browser) return null;
  try {
    const raw = localStorage.getItem(PROFILE_CACHE_KEY);
    return raw ? (JSON.parse(raw) as UserProfile) : null;
  } catch {
    return null;
  }
}

function writeCachedProfile(profile: UserProfile | null) {
  if (!browser) return;
  try {
    if (profile) {
      localStorage.setItem(PROFILE_CACHE_KEY, JSON.stringify(profile));
    } else {
      localStorage.removeItem(PROFILE_CACHE_KEY);
    }
  } catch {
    // Ignore cache failures and fall back to live fetches.
  }
}

const { subscribe, set } = writable<UserProfile | null>(readCachedProfile());

let workspaceCleanup: (() => void) | null = null;

function stopWorkspaceWatch() {
  if (workspaceCleanup) {
    workspaceCleanup();
    workspaceCleanup = null;
  }
}

async function refreshProfile() {
  const token = getToken();
  if (!token) {
    set(null);
    writeCachedProfile(null);
    stopWorkspaceWatch();
    return;
  }

  const result = await getUserProfile(token);
  if (result.data) {
    set(result.data);
    writeCachedProfile(result.data);
  } else {
    set(null);
    writeCachedProfile(null);
  }

  if (!workspaceCleanup) {
    workspaceCleanup = watchWorkspaceEvents(token, async (event) => {
      if (event.kind === "profile_updated" || event.kind === "github_accounts_changed") {
        const refreshed = await getUserProfile(token);
        if (refreshed.data) {
          set(refreshed.data);
          writeCachedProfile(refreshed.data);
        }
      }
    });
  }
}

export const profile = { subscribe };

export function useProfileBootstrap() {
  onMount(() => {
    void refreshProfile();

    const handleAuthChange = () => {
      void refreshProfile();
    };

    window.addEventListener("mikrom-auth-change", handleAuthChange);

    return () => {
      window.removeEventListener("mikrom-auth-change", handleAuthChange);
      stopWorkspaceWatch();
    };
  });
}
