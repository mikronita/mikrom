import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import CreateDatabaseModal from "$lib/components/CreateDatabaseModal.svelte";

const mocks = vi.hoisted(() => ({
  createDatabase: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
}));

vi.mock("$lib/stores/databases", () => ({
  createDatabase: mocks.createDatabase,
}));

vi.mock("$lib/components", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/components")>();
  const { default: MockModal } = await import("./ModalFixture.svelte");

  return {
    ...actual,
    Modal: MockModal,
  };
});

vi.mock("$lib/toast", () => ({
  toast: {
    success: mocks.success,
    error: mocks.error,
  },
}));

describe("CreateDatabaseModal", () => {
  it("submits the selected plan with the entered database details", async () => {
    const onClose = vi.fn();
    mocks.createDatabase.mockResolvedValue({
      data: {
        id: "db-3",
        name: "analytics",
        engine: "neon",
        postgres_version: 16,
        status: "Provisioning",
        vcpus: 4,
        memory_mib: 8192,
        disk_mib: 25600,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
      },
    });

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
      expect(mocks.createDatabase).toHaveBeenCalledWith({
        name: "analytics",
        engine: "neon",
        postgres_version: 16,
        vcpus: 4,
        memory_mib: 8192,
        disk_mib: 25600,
      });
    });

    expect(mocks.success).toHaveBeenCalledWith("Database analytics is being provisioned");
    expect(onClose).toHaveBeenCalled();
  });
});
