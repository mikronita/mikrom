import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import CreateDatabaseModal from "$lib/components/CreateDatabaseModal.svelte";

const mocks = vi.hoisted(() => ({
  addDatabase: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
}));

vi.mock("$lib/stores/databases", () => ({
  addDatabase: mocks.addDatabase,
}));

vi.mock("$lib/toast", () => ({
  toast: {
    success: mocks.success,
    error: mocks.error,
  },
}));

describe("CreateDatabaseModal", () => {
  it("submits the selected plan with the entered database details", async () => {
    const onClose = vi.fn();

    render(CreateDatabaseModal, {
      props: {
        open: true,
        onClose,
      },
    });

    await fireEvent.input(screen.getByLabelText("Database Name"), {
      target: { value: "analytics" },
    });
    await fireEvent.input(screen.getByLabelText("Storage (GB)"), {
      target: { value: "25" },
    });
    await fireEvent.click(screen.getByLabelText("Dedicated 4 vCPU / 8GB RAM"));
    await fireEvent.click(screen.getByRole("button", { name: "Provision Database" }));

    await waitFor(() => {
      expect(mocks.addDatabase).toHaveBeenCalledWith({
        name: "analytics",
        version: "16",
        vcpus: 4,
        memory_mib: 8192,
        storage_gb: 25,
      });
    });

    expect(mocks.success).toHaveBeenCalledWith("Database analytics is being provisioned");
    expect(onClose).toHaveBeenCalled();
  });
});
