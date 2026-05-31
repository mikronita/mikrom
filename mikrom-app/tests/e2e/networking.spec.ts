import { expect, test } from "@playwright/test";
import { installBrowserShims, mockControlPlaneApi, seedAuth } from "./helpers";

test.describe("networking", () => {
  test.beforeEach(async ({ page }) => {
    await installBrowserShims(page);
    await seedAuth(page);
    await mockControlPlaneApi(page);
  });

  test("shows mesh status and creates a security rule", async ({ page }) => {
    await page.goto("/networking");

    await expect(page.getByRole("heading", { name: "Networking" })).toBeVisible();
    await expect(page.getByText("Active peers")).toBeVisible();
    await expect(page.getByText("Running workloads")).toBeVisible();

    await page.locator('[data-slot="select-trigger"]').first().click();
    await page.getByRole("option", { name: "starter" }).click();

    await expect(page.getByRole("button", { name: "Add rule" })).toBeVisible();
    await page.getByRole("button", { name: "Add rule" }).click();

    await expect(page.getByRole("heading", { name: "Add security rule" })).toBeVisible();
    await page.locator('input[type="number"]').nth(0).fill("8080");
    await page.locator('input[type="number"]').nth(1).fill("8080");
    await page.getByRole("button", { name: "Create rule" }).click();

    await expect(page.getByText("TCP 8080")).toBeVisible();
  });
});
