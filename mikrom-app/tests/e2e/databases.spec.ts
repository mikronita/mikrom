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

    await page.getByRole("button", { name: "New Database" }).click();
    await expect(page.getByRole("heading", { name: "Create New Database" })).toBeVisible();

    await page.getByLabel("Database Name").fill("analytics");
    await page.getByLabel("Storage (GB)").fill("25");
    await page.locator("label").filter({ hasText: "Dedicated 4 vCPU / 8GB RAM" }).click();
    await page.getByRole("button", { name: "Provision Database" }).click();

    await expect(page.locator('[role="dialog"]')).toHaveCount(0);
    await expect(page.getByRole("link", { name: /analytics.*PostgreSQL 16/i })).toBeVisible();
    await expect(page.getByText("Provisioning")).toBeVisible();
  });
});
