<script lang="ts">
  import { cn } from "$lib/utils";

  type Point = {
    time: string;
    cpu: number;
    ram: number;
    rx: number;
    tx: number;
  };

  type SeriesKey = keyof Omit<Point, "time">;

  type SeriesConfig = {
    key: SeriesKey;
    label: string;
    color: string;
    format: (value: number) => string;
  };

  let { points = [], class: className = "" } = $props<{
    points?: Point[];
    class?: string;
  }>();

  const width = 960;
  const height = 320;
  const topPad = 16;
  const rightPad = 1.25;
  const bottomPad = 40;
  const leftPad = 1.25;
  const plotLeft = leftPad;
  const plotRight = width - rightPad;
  const plotTop = topPad;
  const plotBottom = height - bottomPad;
  const plotWidth = plotRight - plotLeft;
  const plotHeight = plotBottom - plotTop;

  const series: SeriesConfig[] = [
    {
      key: "cpu",
      label: "CPU",
      color: "var(--color-chart-1)",
      format: (value) => `${value.toFixed(1)}%`,
    },
    {
      key: "ram",
      label: "RAM",
      color: "var(--color-chart-2)",
      format: (value) => `${value.toFixed(0)} MiB`,
    },
    {
      key: "rx",
      label: "Network In",
      color: "var(--color-chart-3)",
      format: formatNetworkRate,
    },
    {
      key: "tx",
      label: "Network Out",
      color: "var(--color-chart-4)",
      format: formatNetworkRate,
    },
  ];

  let hoveredIndex = $state<number | null>(null);
  let chartEl = $state<SVGSVGElement | null>(null);

  function clamp(value: number, min: number, max: number) {
    return Math.min(Math.max(value, min), max);
  }

  function formatNetworkRate(kibPerSecond: number) {
    if (!Number.isFinite(kibPerSecond) || kibPerSecond <= 0) return "0 KiB/s";
    if (kibPerSecond < 0.1) return `${(kibPerSecond * 1024).toFixed(0)} B/s`;
    if (kibPerSecond >= 1024) return `${(kibPerSecond / 1024).toFixed(1)} MiB/s`;
    return `${kibPerSecond.toFixed(1)} KiB/s`;
  }

  function normalize(value: number, min: number, max: number) {
    if (!Number.isFinite(value)) return 0;
    if (max <= min) return 0.5;
    return (value - min) / (max - min);
  }

  function buildCurvePath(values: number[], min: number, max: number) {
    if (!values.length) return "";
    const step = plotWidth / Math.max(values.length - 1, 1);
    const coordinates = values.map((value, index) => ({
      x: plotLeft + step * index,
      y: plotBottom - normalize(value, min, max) * plotHeight,
    }));

    if (coordinates.length === 1) {
      return `M ${coordinates[0].x} ${coordinates[0].y}`;
    }

    const path: string[] = [`M ${coordinates[0].x} ${coordinates[0].y}`];

    for (let index = 0; index < coordinates.length - 1; index += 1) {
      const p0 = coordinates[index - 1] ?? coordinates[index];
      const p1 = coordinates[index];
      const p2 = coordinates[index + 1];
      const p3 = coordinates[index + 2] ?? p2;

      const cp1x = p1.x + (p2.x - p0.x) / 6;
      const cp1y = p1.y + (p2.y - p0.y) / 6;
      const cp2x = p2.x - (p3.x - p1.x) / 6;
      const cp2y = p2.y - (p3.y - p1.y) / 6;

      path.push(`C ${cp1x} ${cp1y}, ${cp2x} ${cp2y}, ${p2.x} ${p2.y}`);
    }

    return path.join(" ");
  }

  function buildSeriesBounds(points: Point[], configs: SeriesConfig[]) {
    return Object.fromEntries(
      configs.map((config) => {
        const values = points.map((point) => point[config.key]);
        const min = values.length ? Math.min(...values) : 0;
        const max = values.length ? Math.max(...values) : 0;
        const paddedMin = min === max ? min - 1 : min;
        const paddedMax = min === max ? max + 1 : max;
        return [config.key, { min: paddedMin, max: paddedMax }];
      }),
    ) as Record<SeriesKey, { min: number; max: number }>;
  }

  function buildSeriesPaths(
    points: Point[],
    configs: SeriesConfig[],
    bounds: Record<SeriesKey, { min: number; max: number }>,
  ) {
    return Object.fromEntries(
      configs.map((config) => {
        const seriesValues = points.map((point) => point[config.key]);
        const seriesBounds = bounds[config.key];
        return [config.key, buildCurvePath(seriesValues, seriesBounds.min, seriesBounds.max)];
      }),
    ) as Record<SeriesKey, string>;
  }

  function getTooltipXOffset(index: number | null, totalPoints: number) {
    if (index === null || totalPoints === 0) return "-50%";
    if (index < totalPoints / 4) return "0%";
    if (index > (totalPoints * 3) / 4) return "-100%";
    return "-50%";
  }

  function getTooltipTop(
    activePoint: Point | null,
    configs: SeriesConfig[],
    bounds: Record<SeriesKey, { min: number; max: number }>,
  ) {
    if (!activePoint) return "24px";

    const pointTop = Math.min(
      ...configs.map((config) => {
        const seriesBounds = bounds[config.key];
        return plotBottom - normalize(activePoint[config.key], seriesBounds.min, seriesBounds.max) * plotHeight;
      }),
    );

    return `${Math.max(12, Math.min(plotBottom - 100, pointTop - 24))}px`;
  }

  function hoverFromEvent(event: PointerEvent) {
    if (!points.length || !chartEl) return;
    const rect = chartEl.getBoundingClientRect();
    const xScale = rect.width > 0 ? width / rect.width : 1;
    const x = clamp((event.clientX - rect.left) * xScale, plotLeft, plotRight);
    const relative = (x - plotLeft) / Math.max(plotWidth, 1);
    hoveredIndex = clamp(Math.round(relative * (points.length - 1)), 0, points.length - 1);
  }

  const chartPoints = $derived(points.slice(-30));
  const visiblePoints = $derived(chartPoints);
  const xStep = $derived(visiblePoints.length > 1 ? plotWidth / (visiblePoints.length - 1) : 0);
  const seriesBounds = $derived(buildSeriesBounds(visiblePoints, series));
  const paths = $derived(buildSeriesPaths(visiblePoints, series, seriesBounds));

  const activeIndex = $derived(hoveredIndex);
  const activePoint = $derived(activeIndex !== null ? visiblePoints[activeIndex] : null);
  const activeX = $derived(activeIndex !== null ? plotLeft + xStep * activeIndex : null);

  const tooltipXOffset = $derived(getTooltipXOffset(activeIndex, visiblePoints.length));

  const tooltipLeft = $derived(activeX !== null ? `${(activeX / width) * 100}%` : "0%");
  const tooltipTop = $derived(getTooltipTop(activePoint, series, seriesBounds));

  function clearHover() {
    hoveredIndex = null;
  }
</script>

<div class={cn("overflow-hidden rounded-lg border border-border bg-card shadow-xs", className)}>
  {#if visiblePoints.length > 0}
    <div class="relative pt-3 sm:pt-4">
      <svg
        bind:this={chartEl}
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        role="img"
        aria-label="System performance chart with CPU, RAM, Network In and Network Out"
        class="h-[300px] w-full touch-none overflow-visible sm:h-[320px]"
        onpointermove={hoverFromEvent}
        onpointerleave={clearHover}
        onpointerdown={hoverFromEvent}
      >
        <defs>
          <linearGradient id="chart-grid-fade" x1="0" x2="0" y1="0" y2="1">
            <stop offset="0%" stop-color="var(--border)" stop-opacity="0.55" />
            <stop offset="100%" stop-color="var(--border)" stop-opacity="0.18" />
          </linearGradient>
        </defs>

        <g stroke="url(#chart-grid-fade)" stroke-width="1">
          {#each Array.from({ length: 4 }) as _, index}
            <line
              x1={plotLeft}
              x2={plotRight}
              y1={plotTop + (plotHeight / 3) * index}
              y2={plotTop + (plotHeight / 3) * index}
            />
          {/each}
        </g>

        <g class="fill-muted-foreground text-[10px]">
          {#each visiblePoints as point, index}
            {#if index % Math.max(Math.ceil(visiblePoints.length / 6), 1) === 0 || index === visiblePoints.length - 1}
              <text x={plotLeft + xStep * index} y={height - 12} text-anchor="middle">
                {point.time}
              </text>
            {/if}
          {/each}
        </g>

        {#if activeX !== null}
          <line x1={activeX} x2={activeX} y1={plotTop} y2={plotBottom} stroke="var(--border)" stroke-dasharray="4 4" />
        {/if}

        {#each series as config}
          {@const bounds = seriesBounds[config.key]}
          {#if paths[config.key]}
            <path d={paths[config.key]} fill="none" stroke={config.color} stroke-width="2.25" stroke-linecap="round" stroke-linejoin="round" />
          {/if}

          {#if activePoint && activeIndex !== null}
            {@const value = activePoint[config.key]}
            {@const y = plotBottom - normalize(value, bounds.min, bounds.max) * plotHeight}
            <circle cx={activeX ?? plotLeft} cy={y} r="4.5" fill="var(--background)" stroke={config.color} stroke-width="2.5" />
          {/if}
        {/each}
      </svg>

      {#if activePoint && activeIndex !== null}
        <div
          class="pointer-events-none absolute z-10 min-w-56 -translate-y-full rounded-lg border border-border bg-popover/95 px-3 py-2 shadow-lg backdrop-blur"
          style={`left: ${tooltipLeft}; top: ${tooltipTop}; transform: translate(${tooltipXOffset}, -100%);`}
        >
          <div class="text-xs font-medium text-muted-foreground">{activePoint.time}</div>
          <div class="mt-2 grid gap-2">
            {#each series as config}
              <div class="flex items-center justify-between gap-4 text-xs sm:text-sm">
                <div class="flex items-center gap-2">
                  <span class="size-2 rounded-full" style={`background-color: ${config.color}`}></span>
                  <span class="text-muted-foreground">{config.label}</span>
                </div>
                <span class="font-medium tabular-nums">{config.format(activePoint[config.key])}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  {:else}
    <div class="flex h-[320px] items-center justify-center px-6 text-center">
      <div class="flex max-w-sm flex-col gap-2">
        <div class="text-sm font-medium">No metrics yet</div>
        <p class="text-sm text-muted-foreground">Live CPU, RAM and network samples will appear here once the deployment starts reporting data.</p>
      </div>
    </div>
  {/if}

  <div class="flex flex-col gap-3 border-t border-border px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
    <div class="text-sm text-muted-foreground">
      Showing the latest {Math.min(visiblePoints.length, 30)} samples for the active deployment
    </div>
    <div class="flex flex-wrap items-center gap-x-5 gap-y-2">
      {#each series as config}
        {@const latest = visiblePoints.at(-1)}
        <div class="flex items-center gap-2 text-xs text-muted-foreground">
          <span class="size-2 rounded-full" style={`background-color: ${config.color}`}></span>
          <span>{config.label}</span>
          {#if latest}
            <span class="font-medium text-foreground tabular-nums">{config.format(latest[config.key])}</span>
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>
