import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UserProfile } from "$lib/api";

const mocks = vi.hoisted(() => ({
  getToken: vi.fn(),
  getUserProfile: vi.fn(),
}));

vi.mock("$lib/auth", () => ({
  getToken: mocks.getToken,
}));

vi.mock("$lib/api", () => ({
  getUserProfile: mocks.getUserProfile,
}));

const cachedProfile: UserProfile = {
  id: "user-1",
  email: "alice@mikrom.io",
  role: "admin",
  first_name: "Alice",
  last_name: "Example",
  avatar_url: null,
  vpc_ipv6_prefix: "fd00:1234::/40",
};

const updatedProfile: UserProfile = {
  ...cachedProfile,
  first_name: "Ada",
  last_name: "Lovelace",
};

beforeEach(() => {
  vi.resetModules();
  localStorage.clear();
  mocks.getToken.mockReset();
  mocks.getUserProfile.mockReset();
});

describe("profile store", () => {
  it("hydrates from cached profile data and refreshes it", async () => {
    localStorage.setItem("mikrom_profile", JSON.stringify(cachedProfile));

    const { profile, refreshProfile } = await import("$lib/stores/profile");

    let current: UserProfile | null = null;
    const unsubscribe = profile.subscribe((value) => {
      current = value;
    });

    expect(current).toEqual(cachedProfile);

    mocks.getToken.mockReturnValue("token");
    mocks.getUserProfile.mockResolvedValue({ data: updatedProfile });

    await refreshProfile();

    expect(current).toEqual(updatedProfile);
    expect(JSON.parse(localStorage.getItem("mikrom_profile") || "null")).toEqual(updatedProfile);

    unsubscribe();
  });

  it("clears the cache when there is no active session", async () => {
    localStorage.setItem("mikrom_profile", JSON.stringify(cachedProfile));

    const { refreshProfile } = await import("$lib/stores/profile");

    mocks.getToken.mockReturnValue(null);

    await refreshProfile();

    expect(localStorage.getItem("mikrom_profile")).toBeNull();
  });
});
