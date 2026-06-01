import { describe, expect, it } from "vitest";
import {
  aggregateReplicaMetrics,
  buildMetricSnapshot,
  formatBytes,
  formatDeploymentDate,
  formatNetworkRate,
  getDeploymentBadgeProps,
  getDeploymentButtonText,
  normalizeCpuUsage,
} from "$lib/domain/app-details";

describe("app detail helpers", () => {
  it("formats resource values", () => {
    expect(normalizeCpuUsage(0.25)).toBe(25);
    expect(formatNetworkRate(0)).toBe("0 KiB/s");
    expect(formatBytes(1536)).toBe("1.5 KiB");
    expect(formatDeploymentDate("2026-05-04T12:30:00.000Z")).toContain("2026");
  });

  it("maps deployment status to button and badge states", () => {
    expect(getDeploymentBadgeProps("RUNNING")).toEqual({
      variant: "outline",
      className: "border-transparent bg-status-info/10 text-status-info",
    });
    expect(getDeploymentBadgeProps("FAILED")).toEqual({
      variant: "destructive",
      className: "",
    });
    expect(getDeploymentButtonText({ status: "SCHEDULED" } as never, false)).toBe("Starting...");
  });

  it("builds and aggregates metrics snapshots", () => {
    const snapshot = buildMetricSnapshot(
      {
        cpu_usage: 0.5,
        ram_used_bytes: 1024 * 1024,
        tx_bytes: 2048,
        rx_bytes: 4096,
      } as never,
      10_000,
      { tx: 1024, rx: 2048, time: 8_000, txRate: 0, rxRate: 0 },
    );

    expect(snapshot.sample.cpu).toBe(50);
    expect(snapshot.sample.ram).toBe(1);
    expect(snapshot.cache.tx).toBe(2048);

    const aggregate = aggregateReplicaMetrics([
      { cpu: 20, ram: 128, rx: 3, tx: 4, total_rx: 5, total_tx: 6 },
      { cpu: 40, ram: 256, rx: 1, tx: 2, total_rx: 7, total_tx: 8 },
    ]);

    expect(aggregate.cpu).toBe(30);
    expect(aggregate.ram).toBe(384);
    expect(aggregate.total_rx).toBe(12);
  });

  it("returns zeroed aggregate metrics for an empty replica set", () => {
    const aggregate = aggregateReplicaMetrics([]);

    expect(aggregate.cpu).toBe(0);
    expect(aggregate.ram).toBe(0);
    expect(aggregate.rx).toBe(0);
    expect(aggregate.tx).toBe(0);
    expect(aggregate.total_rx).toBe(0);
    expect(aggregate.total_tx).toBe(0);
  });
});
