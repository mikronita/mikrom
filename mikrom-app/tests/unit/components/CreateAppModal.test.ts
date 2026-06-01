import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import CreateAppModal from "$lib/components/CreateAppModal.svelte";
import { setActiveProjectSlug } from "$lib/stores/projects";

const mocks = vi.hoisted(() => ({
  createApp: vi.fn(),
  getGithubInstallUrl: vi.fn(),
  listGithubRepos: vi.fn(),
  getToken: vi.fn(),
  goto: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
  loading: vi.fn(),
  dismiss: vi.fn(),
}));

beforeEach(() => {
  setActiveProjectSlug(null);
});

vi.mock("$lib/api", () => ({
  createApp: mocks.createApp,
  getGithubInstallUrl: mocks.getGithubInstallUrl,
  listGithubRepos: mocks.listGithubRepos,
}));

vi.mock("$lib/components", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/components")>();
  const { default: MockModal } = await import("./ModalFixture.svelte");

  return {
    ...actual,
    Modal: MockModal,
  };
});

vi.mock("$lib/auth", () => ({
  getToken: mocks.getToken,
}));

vi.mock("$app/navigation", () => ({
  goto: mocks.goto,
}));

vi.mock("$lib/toast", () => ({
  toast: {
    success: mocks.success,
    error: mocks.error,
    loading: mocks.loading,
    dismiss: mocks.dismiss,
  },
}));

describe("CreateAppModal", () => {
  it("creates an application from a manual Git URL", async () => {
    const onClose = vi.fn();

    mocks.getToken.mockReturnValue("token");
    mocks.listGithubRepos.mockResolvedValue({ data: [] });
    mocks.createApp.mockResolvedValue({ data: {} });
    setActiveProjectSlug("acme");

    render(CreateAppModal, {
      props: {
        open: true,
        onClose,
      },
    });

    await fireEvent.input(screen.getByLabelText("App Name"), {
      target: { value: "starter" },
    });
    await fireEvent.input(screen.getByLabelText("Git Repository URL"), {
      target: { value: "https://github.com/mikrom/starter" },
    });
    await fireEvent.click(screen.getByRole("button", { name: "Create App" }));

    await waitFor(() => {
      expect(mocks.createApp).toHaveBeenCalledWith("token", {
        name: "starter",
        git_url: "https://github.com/mikrom/starter",
      });
    });

    expect(mocks.success).toHaveBeenCalledWith("App starter created successfully");
    expect(onClose).toHaveBeenCalled();
    expect(mocks.goto).not.toHaveBeenCalled();
  });
});
