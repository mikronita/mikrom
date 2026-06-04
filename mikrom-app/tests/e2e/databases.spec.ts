import { expect, test } from "@playwright/test";
import { installBrowserShims, mockControlPlaneApi, seedAuth } from "./helpers";

test.describe("databases", () => {
  test.beforeEach(async ({ page }) => {
    await installBrowserShims(page);
    await seedAuth(page);
    await mockControlPlaneApi(page);
  });

  test("creates a new database from the dashboard modal", async ({ page }) => {
    await page.goto("/databases");

    await expect(page.getByRole("heading", { name: "Databases" })).toBeVisible();
    await expect(page.getByRole("link", { name: /orders-db.*PostgreSQL 16/i })).toBeVisible();

    await page.getByRole("button", { name: "New Database" }).click();
    await expect(page.getByRole("heading", { name: "Create New Database" })).toBeVisible();

    await page.getByLabel("Database Name").fill("analytics");
    await page.getByLabel("Storage (GB)").fill("25");
    await page.locator("label").filter({ hasText: "Dedicated 4 vCPU / 8GB RAM" }).click();
    await page.getByRole("button", { name: "Provision Database" }).click();

    await expect(page.locator('[role="dialog"]')).toHaveCount(0);
    const createdCard = page.getByRole("link", { name: /analytics.*PostgreSQL 16/i });
    await expect(createdCard).toBeVisible();
    await expect(createdCard.getByText("Provisioning")).toBeVisible();
  });

  test("shows the connection flow for an existing database", async ({ page }) => {
    await page.goto("/databases/orders-db");

    await expect(page.getByRole("heading", { name: "orders-db" })).toBeVisible();
    await expect(page.getByText("PostgreSQL 16", { exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Connection", exact: true }).click();
    await expect(page.getByText("SSH tunnel command")).toBeVisible();
    await expect(page.getByText("ssh -N -L 5432:127.0.0.1:5432 mikrom@[fd00:1234::99]")).toBeVisible();
    await expect(page.getByText("psql command")).toBeVisible();
    await expect(page.getByText('psql "host=127.0.0.1 port=5432 user=cloud_admin dbname=orders-db"')).toBeVisible();
  });

  test("creates, restores, and deletes a database snapshot from the backups tab", async ({ page }) => {
    await page.goto("/databases/orders-db");

    await page.getByRole("button", { name: "Backups" }).click();
    await expect(page.getByRole("button", { name: "Create Snapshot" })).toBeVisible();
    await expect(page.getByText("Snapshot Actions")).toBeVisible();

    const snapshotName = "nightly-2026-06-04";
    const createRequest = page.waitForRequest((request) =>
      request.url().includes("/api/v1/databases/db-1/backups/snapshots") && request.method() === "POST"
    );
    await page.getByRole("textbox").fill(snapshotName);
    await page.getByRole("button", { name: "Create Snapshot" }).click();
    await createRequest;
    await expect(page.getByText(snapshotName)).toBeVisible();

    const restoreRequest = page.waitForRequest((request) =>
      request.url().includes("/api/v1/databases/db-1/backups/restore") && request.method() === "POST"
    );
    await page.getByRole("button", { name: "Restore", exact: true }).click();
    const restoreDialog = page.getByRole("alertdialog", { name: "Restore snapshot?" });
    await expect(restoreDialog).toBeVisible();
    await restoreDialog.getByRole("button", { name: "Restore Snapshot", exact: true }).click();
    await restoreRequest;
    await expect(restoreDialog).toHaveCount(0);

    const deleteRequest = page.waitForRequest((request) =>
      request.url().includes(`/api/v1/databases/db-1/backups/snapshots/${snapshotName}`) &&
      request.method() === "DELETE"
    );
    await page.getByRole("button", { name: "Delete", exact: true }).click();
    const deleteDialog = page.getByRole("alertdialog", { name: "Delete snapshot?" });
    await expect(deleteDialog).toBeVisible();
    await deleteDialog.getByRole("button", { name: "Delete Snapshot", exact: true }).click();
    await deleteRequest;

    await page.reload();
    await page.getByRole("button", { name: "Backups" }).click();
    await expect(page.getByRole("button", { name: "Create Snapshot" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Restore", exact: true })).toHaveCount(0, {
      timeout: 6000,
    });
  });

  test("shows an error when snapshot operations are attempted without an active deployment", async ({ page }) => {
    await page.goto("/databases/shadow-db");

    await expect(page.getByRole("heading", { name: "shadow-db" })).toBeVisible();
    await page.getByRole("button", { name: "Backups" }).click();

    await expect(
      page.getByText(
        "This database needs an active deployment before you can use snapshots. Provision or deploy it first."
      ).first()
    ).toBeVisible();

    await page.getByRole("textbox").fill("manual-check");
    await page.getByRole("button", { name: "Create Snapshot" }).click();

    await expect(
      page.getByText(
        "This database needs an active deployment before you can use snapshots. Provision or deploy it first."
      ).first()
    ).toBeVisible();
  });
});
