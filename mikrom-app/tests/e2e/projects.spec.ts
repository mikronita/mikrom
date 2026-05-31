import { expect, test } from "@playwright/test";
import { installBrowserShims, mockControlPlaneApi, seedAuth } from "./helpers";

test.describe("projects", () => {
  test.beforeEach(async ({ page }) => {
    await installBrowserShims(page);
    await seedAuth(page);
    await mockControlPlaneApi(page);
  });

  test("creates, renames and deletes an empty project", async ({ page }) => {
    await page.goto("/projects");

    await expect(page.getByRole("heading", { name: "Projects" })).toBeVisible();
    await page.getByPlaceholder("Search by project name or slug").fill("missing");
    await expect(page.getByText("No matching projects")).toBeVisible();
    await page.getByPlaceholder("Search by project name or slug").fill("");
    await page.getByLabel("Project name").fill("Temporary Project");
    await page.getByRole("button", { name: "Create project" }).click();

    await expect(page).toHaveURL("/");

    await page.goto("/projects");
    const projectCard = page.locator('[data-project-slug="proj02"]');
    await expect(projectCard.getByText("Temporary Project")).toBeVisible();

    await projectCard.getByRole("button", { name: "Rename" }).click();
    const renameDialog = page.getByRole("dialog", { name: "Rename project" });
    await expect(renameDialog).toBeVisible();
    await renameDialog.getByLabel("Project name").fill("Renamed Project");
    await renameDialog.getByRole("button", { name: "Save changes" }).click();

    await expect(projectCard.getByText("Renamed Project")).toBeVisible();

    await projectCard.getByRole("button", { name: "Delete" }).click();
    const deleteDialog = page.getByRole("alertdialog", { name: "Delete project?" });
    await expect(deleteDialog).toBeVisible();
    await deleteDialog.getByRole("button", { name: "Delete project" }).click();

    await expect(page.locator('[data-project-slug="proj02"]')).toHaveCount(0);
  });

  test("blocks deleting a project that still has dependent resources", async ({ page }) => {
    await page.goto("/projects");

    const projectCard = page.locator('[data-project-slug="acme"]');
    const deleteResponse = page.waitForResponse((response) =>
      response.url().includes("/api/v1/projects/acme") &&
      response.request().method() === "DELETE"
    );

    await projectCard.getByRole("button", { name: "Delete" }).click();
    const deleteDialog = page.getByRole("alertdialog", { name: "Delete project?" });
    await expect(deleteDialog).toBeVisible();
    await deleteDialog.getByRole("button", { name: "Delete project" }).click();

    const response = await deleteResponse;
    expect(response.status()).toBe(409);
    await expect(deleteDialog).toBeVisible();
    await expect(page.getByText("This project still has apps, databases or volumes. Remove them first.")).toBeVisible();
  });
});
