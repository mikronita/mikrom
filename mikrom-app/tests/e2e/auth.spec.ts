import { expect, test } from "@playwright/test";
import { authToken, installBrowserShims, mockControlPlaneApi } from "./helpers";

test.describe("authentication", () => {
  test.beforeEach(async ({ page }) => {
    await installBrowserShims(page);
    await mockControlPlaneApi(page);
  });

  test("renders the login form and allows access to the dashboard when authenticated", async ({ page }) => {
    await page.goto("/auth/login");

    await expect(page.getByRole("heading", { name: "Sign in to Mikrom" })).toBeVisible();
    await expect(page.getByLabel("Email address")).toBeVisible();
    await expect(page.getByLabel("Password")).toBeVisible();

    await page.evaluate((token) => localStorage.setItem("mikrom_token", token), authToken);
    await page.goto("/");

    await expect(page).toHaveURL("/");
    await expect(page.getByRole("heading", { name: "Dashboard" })).toBeVisible();
  });

  test("renders the registration form", async ({ page }) => {
    await page.goto("/auth/register");

    await expect(page.getByRole("heading", { name: "Create your Mikrom account" })).toBeVisible();
    await expect(page.getByLabel("Email address")).toBeVisible();
    await expect(page.locator("#password")).toBeVisible();
    await expect(page.locator("#confirmPassword")).toBeVisible();
  });
});
