import { describe, expect, it } from "vitest";
import {
  buildAppCards,
  buildDashboardSummary,
  filterAppCards,
  getSystemHealthStatus,
  sortByCreatedAtDesc,
} from "$lib/domain/ui";

const apps = [
  {
    id: "app-1",
    name: "starter",
    git_url: "https://github.com/mikrom/starter",
    port: 3000,
    hostname: "starter.mikrom.dev",
    active_deployment_id: "deploy-1",
    desired_replicas: 1,
    min_replicas: 1,
    max_replicas: 1,
    autoscaling_enabled: false,
    cpu_threshold: 80,
    mem_threshold: 80,
    scale_state: "active" as const,
    created_at: "2026-05-01T10:00:00.000Z",
    updated_at: "2026-05-03T10:00:00.000Z",
  },
  {
    id: "app-2",
    name: "paused-app",
    git_url: "https://github.com/mikrom/paused",
    port: 3001,
    hostname: null,
    active_deployment_id: null,
    desired_replicas: 0,
    min_replicas: 0,
    max_replicas: 1,
    autoscaling_enabled: false,
    cpu_threshold: 80,
    mem_threshold: 80,
    scale_state: "scaled_to_zero" as const,
    created_at: "2026-05-02T10:00:00.000Z",
  },
];

const vms = [
  {
    job_id: "job-1",
    deployment_id: "deploy-1",
    app_id: "app-1",
    app_name: "starter",
    image: "ghcr.io/mikrom/starter:latest",
    status: "RUNNING",
    host_id: "host-1",
    vm_id: "vm-1",
    cpu_usage: 20,
    ram_used_bytes: 128,
    vcpus: 2,
    memory_mib: 256,
  },
  {
    job_id: "job-2",
    deployment_id: "deploy-2",
    app_id: "app-1",
    app_name: "starter",
    image: "ghcr.io/mikrom/starter:latest",
    status: "PENDING",
    host_id: "host-1",
    vm_id: "vm-2",
    cpu_usage: 0,
    ram_used_bytes: 0,
  },
];

describe("ui selectors", () => {
  it("sorts entities by newest first", () => {
    expect(sortByCreatedAtDesc(apps).map((app) => app.name)).toEqual([
      "paused-app",
      "starter",
    ]);
  });

  it("builds app cards with precomputed resource summaries and filters", () => {
    const cards = buildAppCards(apps, vms);

    expect(cards[0].name).toBe("paused-app");
    expect(cards[1].resources).toEqual({
      vcpus: 2,
      memory_mib: 256,
      count: 1,
    });
    expect(cards[1].filterState).toBe("active");
    expect(cards[1].scaleLabel).toBe("Running");
    expect(cards[1].searchText).toContain("starter.mikrom.dev");

    expect(filterAppCards(cards, "github.com/mikrom/paused", "all").map((card) => card.name)).toEqual([
      "paused-app",
    ]);
    expect(filterAppCards(cards, "", "paused").map((card) => card.name)).toEqual([
      "paused-app",
    ]);
  });

  it("builds dashboard summary in a single pass", () => {
    const summary = buildDashboardSummary(apps, vms);

    expect(summary.totalApps).toBe(2);
    expect(summary.runningCount).toBe(1);
    expect(summary.pendingCount).toBe(1);
    expect(summary.hasUndeployedApps).toBe(false);
    expect(summary.recentApps[0]).toMatchObject({
      name: "paused-app",
      status: "Stopped",
      statusVariant: "outline",
      statusClass: "border-transparent bg-muted/40 text-muted-foreground/60",
    });
  });

  it("maps health status to UI states", () => {
    expect(getSystemHealthStatus(null)).toBe("OFFLINE");
    expect(getSystemHealthStatus({ status: "ok", version: "1.0.0", services: {} })).toBe("CHECKING");
    expect(
      getSystemHealthStatus({
        status: "ok",
        version: "1.0.0",
        services: { API: "ONLINE", Router: "ONLINE" },
      }),
    ).toBe("ONLINE");
  });
});
