import { expect, test } from "@playwright/test";
import { authToken, installBrowserShims, mockControlPlaneApi, seedAuth } from "./helpers";

test.describe("detail pages", () => {
  test.beforeEach(async ({ page }) => {
    await installBrowserShims(page);
    await seedAuth(page, authToken);
    await mockControlPlaneApi(page);
  });

  test("shows app deployment details and live performance scaffolding", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByRole("heading", { name: "Dashboard" })).toBeVisible();

    await page.goto("/apps/starter");

    await expect(page.getByRole("heading", { name: "starter" })).toBeVisible();
    await expect(page.getByText("Updated May 4, 2026", { exact: true })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Deployment History" })).toBeVisible();
    await expect(page.getByText("Initial stable release")).toBeVisible();
    await expect(page.getByText("Production", { exact: true })).toBeVisible();

    const deployRequest = page.waitForRequest((request) =>
      request.url().includes("/api/v1/apps/starter/deploy") && request.method() === "POST"
    );
    await page.getByRole("button", { name: "Deploy Now" }).click();
    const deployDialog = page.getByRole("dialog", { name: "Deploy starter" });
    await expect(deployDialog).toBeVisible();
    await deployDialog.getByRole("button", { name: "Deploy", exact: true }).click();
    await deployRequest;
    await expect(page.locator('[role="dialog"]')).toHaveCount(0);

    await page.getByRole("button", { name: "Auto-deploy" }).click();
    await expect(page.getByRole("heading", { name: "GitHub Auto-deploy Configuration" })).toBeVisible();
    await expect(page.locator('input[readonly]').first()).toHaveValue(/\/webhooks\/github\/starter$/);
    await expect(page.locator('input[type="password"]').first()).toHaveValue("secret-123");
    await page.keyboard.press("Escape");
    await expect(page.locator('[role="dialog"]')).toHaveCount(0);

    const activateRequest = page.waitForRequest((request) =>
      request.url().includes("/api/v1/apps/starter/deployments/deploy-2/activate") &&
      request.method() === "POST"
    );
    await page.getByRole("button", { name: "Promote to Prod" }).click();
    await activateRequest;
    await page.getByRole("button", { name: "Scaling" }).click();
    await expect(page.getByRole("heading", { name: /Scaling & Reliability:/i })).toBeVisible();
  });

  test("deletes the app and returns to the applications list", async ({ page }) => {
    await page.goto("/apps/starter");

    const deleteRequest = page.waitForRequest((request) =>
      request.url().includes("/api/v1/apps/starter") && request.method() === "DELETE"
    );
    await page.getByRole("button", { name: "Delete App", exact: true }).click();
    const deleteDialog = page.getByRole("alertdialog", { name: "Delete application?" });
    await expect(deleteDialog).toBeVisible();
    await deleteDialog.getByRole("button", { name: "Delete App", exact: true }).click();
    await deleteRequest;

    await expect(page).toHaveURL("/apps");
    await expect(page.getByText("starter")).toHaveCount(0);
  });

  test("shows database details and lets the operator inspect tabs", async ({ page }) => {
    await page.goto("/databases/prod-db");

    await expect(page.getByRole("heading", { name: "prod-db" })).toBeVisible();
    await expect(page.getByText("PostgreSQL 16")).toBeVisible();
    await expect(page.getByText("Private 6PN", { exact: true })).toBeVisible();

    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByText("Database Configuration", { exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Delete Database" })).toBeVisible();
  });

  test("deletes the database and removes it from the list after the deletion transition", async ({ page }) => {
    await page.goto("/databases/prod-db");

    await page.getByRole("button", { name: "Settings" }).click();
    await page.getByRole("button", { name: "Delete Database", exact: true }).click();

    const deleteDialog = page.getByRole("alertdialog", { name: "Are you absolutely sure?" });
    await expect(deleteDialog).toBeVisible();
    await deleteDialog.getByRole("button", { name: "Delete Database", exact: true }).click();

    await expect(page).toHaveURL("/databases");
    await expect(page.getByRole("button", { name: "Deleting" })).toBeVisible();
    await expect(page.getByRole("link", { name: /prod-db/i })).toHaveCount(0, { timeout: 6000 });
  });

  test("shows storage details and snapshot history", async ({ page }) => {
    await page.goto("/storage");

    await expect(page.getByRole("heading", { name: "Storage" })).toBeVisible();
    await page.getByRole("link", { name: /app-data/i }).click();

    await expect(page.getByRole("heading", { name: "app-data" })).toBeVisible();
    await expect(page.getByText("starter")).toBeVisible();
    await expect(page.getByText("/data (RWO)")).toBeVisible();
    await expect(page.getByRole("button", { name: "Take Snapshot" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Delete" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Detach" })).toBeVisible();

    await page.getByRole("button", { name: "Snapshots" }).click();
    await expect(page.getByText("daily-backup")).toBeVisible();
    await expect(page.getByRole("button", { name: "Create Snapshot" })).toBeVisible();

    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByRole("button", { name: "Delete Volume" })).toBeVisible();

    const deleteRequest = page.waitForRequest((request) =>
      request.url().includes("/api/v1/volumes/vol-1") && request.method() === "DELETE"
    );
    await page.getByRole("button", { name: "Delete Volume", exact: true }).click();
    const deleteDialog = page.getByRole("alertdialog", { name: "Delete volume?" });
    await expect(deleteDialog).toBeVisible();
    await deleteDialog.getByRole("button", { name: "Delete Volume", exact: true }).click();
    await deleteRequest;

    await expect(page).toHaveURL("/storage");
    await expect(page.getByRole("link", { name: /app-data/i })).toHaveCount(0);
  });
});

test.describe("auto-deploy edge cases", () => {
  test.beforeEach(async ({ page }) => {
    await installBrowserShims(page);
    await seedAuth(page, authToken);
    await mockControlPlaneApi(page, { githubWebhookSecret: "" });
  });

  test("renders an empty webhook secret field when the backend has not provisioned one", async ({ page }) => {
    await page.goto("/apps/starter");

    await page.getByRole("button", { name: "Auto-deploy" }).click();
    await expect(page.getByRole("heading", { name: "GitHub Auto-deploy Configuration" })).toBeVisible();
    await expect(page.locator('input[readonly]').first()).toHaveValue(/\/webhooks\/github\/starter$/);
    await expect(page.locator('input[type="password"]').first()).toHaveValue("");
  });
});
