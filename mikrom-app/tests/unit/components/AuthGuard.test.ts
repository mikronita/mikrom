import { describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/svelte";
import AuthGuard from "$lib/components/AuthGuard.svelte";

const mocks = vi.hoisted(() => ({
  goto: vi.fn(),
  isAuthenticated: vi.fn(),
}));

vi.mock("$app/navigation", () => ({
  goto: mocks.goto,
}));

vi.mock("$lib/auth", () => ({
  isAuthenticated: mocks.isAuthenticated,
}));

describe("AuthGuard", () => {
  it("redirects anonymous users to the login page", async () => {
    mocks.isAuthenticated.mockReturnValue(false);

    render(AuthGuard, {
      props: {
        children: () => "Protected content",
      },
    });

    await waitFor(() => {
      expect(mocks.goto).toHaveBeenCalledWith("/auth/login");
    });

    expect(screen.queryByText("Protected content")).toBeNull();
  });

  it("renders protected content for authenticated users", () => {
    mocks.isAuthenticated.mockReturnValue(true);

    render(AuthGuard, {
      props: {
        children: () => "Protected content",
      },
    });

    expect(mocks.goto).not.toHaveBeenCalled();
    expect(screen.queryByText("Protected content")).toBeNull();
  });
});
