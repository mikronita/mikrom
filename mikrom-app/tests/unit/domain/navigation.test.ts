import { describe, expect, it } from "vitest";
import { buildBreadcrumbs, getRouteName } from "$lib/domain/navigation";

describe("navigation helpers", () => {
  it("builds breadcrumbs from a pathname", () => {
    expect(buildBreadcrumbs("/apps/starter").map((crumb) => crumb.label)).toEqual([
      "Apps",
      "Starter",
    ]);
    expect(buildBreadcrumbs("/apps/starter")[1]).toMatchObject({
      href: "/apps/starter",
      current: true,
    });
  });

  it("derives the route name from the last path segment", () => {
    expect(getRouteName("/storage/app-data")).toBe("App-data");
  });
});
