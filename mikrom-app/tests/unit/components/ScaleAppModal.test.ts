import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import ScaleAppModal from "$lib/components/ScaleAppModal.svelte";

const mocks = vi.hoisted(() => ({
  scaleApp: vi.fn(),
  getToken: vi.fn(),
  refreshApps: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
}));

vi.mock("$lib/api", () => ({
  scaleApp: mocks.scaleApp,
}));

vi.mock("$lib/auth", () => ({
  getToken: mocks.getToken,
}));

vi.mock("$lib/stores/apps", () => ({
  refreshApps: mocks.refreshApps,
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

describe("ScaleAppModal", () => {
  it("updates the scaling configuration for the app", async () => {
    mocks.getToken.mockReturnValue("token");
    mocks.scaleApp.mockResolvedValue({ success: true });

    render(ScaleAppModal, {
      props: {
        open: true,
        app,
      },
    });

    const desiredReplicas = screen.getAllByRole("spinbutton")[0];
    await fireEvent.input(desiredReplicas, { target: { value: "2" } });
    await fireEvent.click(screen.getByRole("button", { name: "Save Configuration" }));

    await waitFor(() => {
      expect(mocks.scaleApp).toHaveBeenCalledWith("token", "starter", {
        desired_replicas: 2,
        min_replicas: 0,
        max_replicas: 2,
        autoscaling_enabled: false,
        cpu_threshold: 80,
        mem_threshold: 80,
      });
    });

    expect(mocks.refreshApps).toHaveBeenCalled();
    expect(mocks.success).toHaveBeenCalledWith("Scaling configuration updated");
  });

  it("saves autoscaling thresholds when autoscaling is enabled", async () => {
    mocks.getToken.mockReturnValue("token");
    mocks.scaleApp.mockResolvedValue({ success: true });

    render(ScaleAppModal, {
      props: {
        open: true,
        app,
      },
    });

    await fireEvent.click(screen.getByRole("switch"));

    const inputs = screen.getAllByRole("spinbutton");
    await fireEvent.input(inputs[0], { target: { value: "3" } });
    await fireEvent.input(inputs[1], { target: { value: "65" } });
    await fireEvent.input(inputs[2], { target: { value: "70" } });
    await fireEvent.click(screen.getByRole("button", { name: "Save Configuration" }));

    await waitFor(() => {
      expect(mocks.scaleApp).toHaveBeenCalledWith("token", "starter", {
        desired_replicas: 1,
        min_replicas: 0,
        max_replicas: 3,
        autoscaling_enabled: true,
        cpu_threshold: 65,
        mem_threshold: 70,
      });
    });
  });
});
