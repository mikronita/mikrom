import { expect, test } from "@playwright/test";
import { authToken, installBrowserShims, mockControlPlaneApi } from "./helpers";

test.describe("sidebar persistence", () => {
  test.beforeEach(async ({ page }) => {
    await installBrowserShims(page);
    await mockControlPlaneApi(page);
    await page.addInitScript((token) => localStorage.setItem("mikrom_token", token), authToken);
  });

  test("preserves the collapsed state across navigation and reloads", async ({ page }) => {
    await page.goto("/");

    const sidebar = page.locator('aside[data-variant="sidebar"]');
    const toggle = page.locator('header button[aria-label="Toggle Sidebar"]');

    await expect(sidebar).toHaveAttribute("data-state", "expanded");

    await toggle.click();
    await expect(sidebar).toHaveAttribute("data-state", "collapsed");

    await toggle.click();
    await expect(sidebar).toHaveAttribute("data-state", "expanded");

    await page.locator('aside a[href="/apps"]').click();
    await expect(page).toHaveURL("/apps");
    await expect(sidebar).toHaveAttribute("data-state", "expanded");
    await expect(sidebar.getByText("Applications")).toBeVisible();

    await page.reload();
    await expect(sidebar).toHaveAttribute("data-state", "expanded");
    await expect(sidebar.getByText("Applications")).toBeVisible();
  });
});
