import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import DeployAppModal from "$lib/components/DeployAppModal.svelte";

const mocks = vi.hoisted(() => ({
  deployAppVersion: vi.fn(),
  getToken: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
}));

vi.mock("$lib/api", () => ({
  DEPLOYMENT_CPU_OPTIONS: [1, 2, 3, 4],
  DEPLOYMENT_MEMORY_OPTIONS: [
    { label: "512M", value: 512 },
    { label: "1G", value: 1024 },
    { label: "2G", value: 2048 },
    { label: "4G", value: 4096 },
  ],
  DEPLOYMENT_HYPERVISOR_OPTIONS: [
    { label: "Default", value: "" },
    { label: "Firecracker", value: "firecracker" },
    { label: "Cloud Hypervisor", value: "cloud-hypervisor" },
  ],
  deployAppVersion: mocks.deployAppVersion,
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

vi.mock("$lib/toast", () => ({
  toast: {
    success: mocks.success,
    error: mocks.error,
  },
}));

const app = {
  id: "app-1",
  name: "starter",
  git_url: "https://github.com/mikrom/starter",
  port: 3000,
  hostname: null,
  active_deployment_id: null,
  desired_replicas: 1,
  min_replicas: 0,
  max_replicas: 1,
  autoscaling_enabled: false,
  cpu_threshold: 80,
  mem_threshold: 80,
  scale_state: "active" as const,
  created_at: "2026-05-02T10:00:00.000Z",
};

describe("DeployAppModal", () => {
  it("deploys the app with the selected resource preset", async () => {
    mocks.getToken.mockReturnValue("token");
    mocks.deployAppVersion.mockResolvedValue({ data: { status: "scheduled" } });

    render(DeployAppModal, {
      props: {
        open: true,
        app,
      },
    });

    await fireEvent.click(screen.getByRole("button", { name: "Deploy" }));

    await waitFor(() => {
      expect(mocks.deployAppVersion).toHaveBeenCalledWith("token", "starter", {
        vcpus: 1,
        memory_mib: 512,
      });
    });

    expect(mocks.success).toHaveBeenCalledWith("Deployment for starter initiated");
  });
});
