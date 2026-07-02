import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UserProfile } from "$lib/api";
import { getToken } from "$lib/auth";
import { getUserProfile } from "$lib/api";

vi.mock("$lib/auth", () => ({
  getToken: vi.fn(),
}));

vi.mock("$lib/api", () => ({
  getUserProfile: vi.fn(),
}));

const mockedGetToken = vi.mocked(getToken);
const mockedGetUserProfile = vi.mocked(getUserProfile);

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
  mockedGetToken.mockReset();
  mockedGetUserProfile.mockReset();
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

    mockedGetToken.mockReturnValue("token");
    mockedGetUserProfile.mockResolvedValue({ data: updatedProfile });

    await refreshProfile();

    expect(current).toEqual(updatedProfile);
    expect(JSON.parse(localStorage.getItem("mikrom_profile") || "null")).toEqual(updatedProfile);

    unsubscribe();
  });

  it("clears the cache when there is no active session", async () => {
    localStorage.setItem("mikrom_profile", JSON.stringify(cachedProfile));

    const { refreshProfile } = await import("$lib/stores/profile");

    mockedGetToken.mockReturnValue(null);

    await refreshProfile();

    expect(localStorage.getItem("mikrom_profile")).toBeNull();
  });
});
