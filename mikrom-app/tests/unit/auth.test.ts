import { beforeEach, describe, expect, it, vi } from "vitest";
import { getToken, isAuthenticated, removeToken, setToken } from "$lib/auth";

function createToken(payload: Record<string, unknown>) {
  return `header.${Buffer.from(JSON.stringify(payload)).toString("base64")}.signature`;
}

beforeEach(() => {
  localStorage.clear();
});

describe("auth helpers", () => {
  it("returns a valid token before expiry", () => {
    const token = createToken({ exp: Math.floor(Date.now() / 1000) + 60 });
    localStorage.setItem("mikrom_token", token);

    expect(getToken()).toBe(token);
    expect(isAuthenticated()).toBe(true);
  });

  it("removes expired tokens", () => {
    const expired = createToken({ exp: Math.floor(Date.now() / 1000) - 60 });
    localStorage.setItem("mikrom_token", expired);

    expect(getToken()).toBeNull();
    expect(localStorage.getItem("mikrom_token")).toBeNull();
    expect(isAuthenticated()).toBe(false);
  });

  it("stores and clears tokens while emitting auth events", () => {
    const token = createToken({ exp: Math.floor(Date.now() / 1000) + 60 });
    const listener = vi.fn();

    window.addEventListener("mikrom-auth-change", listener);

    setToken(token);
    expect(localStorage.getItem("mikrom_token")).toBe(token);

    removeToken();
    expect(localStorage.getItem("mikrom_token")).toBeNull();
    expect(listener).toHaveBeenCalledTimes(2);

    window.removeEventListener("mikrom-auth-change", listener);
  });
});
