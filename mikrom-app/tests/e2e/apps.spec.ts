import { expect, test } from "@playwright/test";
import { installBrowserShims, mockControlPlaneApi, seedAuth } from "./helpers";

test.describe("applications", () => {
  test.beforeEach(async ({ page }) => {
    await installBrowserShims(page);
    await seedAuth(page);
    await mockControlPlaneApi(page);
  });

  test("opens the create app modal and submits a Git URL", async ({ page }) => {
    await page.goto("/apps");

    await expect(page.getByRole("heading", { name: "Applications" })).toBeVisible();
    await expect(page.getByText("starter")).toBeVisible();

    await page.getByPlaceholder("Search by app name, hostname or repository").fill("missing");
    await expect(page.getByText("No matching applications")).toBeVisible();
    await page.getByPlaceholder("Search by app name, hostname or repository").fill("");
    await expect(page.getByText("starter")).toBeVisible();

    await page.getByRole("button", { name: "New Application" }).click();
    await expect(page.getByRole("heading", { name: "Create New Application" })).toBeVisible();

    await page.getByLabel("App Name").fill("control-plane");
    await page.getByLabel("Git Repository URL").fill("https://github.com/mikrom/control-plane");
    await page.getByRole("button", { name: "Create App" }).click();

    await expect(page.locator('[role="dialog"]')).toHaveCount(0);
    await expect(page).toHaveURL("/apps/control-plane");
  });
});
