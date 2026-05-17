<script lang="ts">
  type Point = {
    time: string;
    cpu: number;
    ram: number;
    rx: number;
    tx: number;
  };

  export let points: Point[] = [];

  const width = 900;
  const height = 320;
  const pad = 16;
  const leftAxisWidth = 42;
  const rightAxisWidth = 58;
  const topPad = 12;
  const bottomPad = 24;
  const plotLeft = leftAxisWidth + pad;
  const plotRight = width - rightAxisWidth - pad;
  const plotTop = topPad;
  const plotBottom = height - bottomPad;
  const plotWidth = plotRight - plotLeft;
  const plotHeight = plotBottom - plotTop;

  function scale(value: number, min: number, max: number, span: number) {
    if (max === min) return span;
    return span - ((value - min) / (max - min)) * span;
  }

  function line(values: number[], maxValue: number) {
    if (!values.length) return "";
    const step = plotWidth / Math.max(values.length - 1, 1);
    return values
      .map((value, index) => `${index === 0 ? "M" : "L"} ${plotLeft + step * index} ${scale(value, 0, maxValue, plotHeight) + plotTop}`)
      .join(" ");
  }

  $: maxRam = Math.max(1024, ...points.map((point) => point.ram));
  $: maxUsage = Math.max(1, ...points.map((point) => point.rx), ...points.map((point) => point.tx));
  $: cpuPath = line(points.map((point) => point.cpu), 100);
  $: ramPath = line(points.map((point) => point.ram), maxRam);
  $: rxPath = line(points.map((point) => point.rx), maxUsage);
  $: txPath = line(points.map((point) => point.tx), maxUsage);

  const leftTicks = [0, 25, 50, 75, 100];
  const rightTicks = [0, 0.25, 0.5, 0.75, 1];

  const legend = [
    { label: "CPU Usage", color: "var(--color-chart-1)" },
    { label: "RAM Usage", color: "var(--color-chart-2)" },
    { label: "Network In", color: "var(--color-chart-3)" },
    { label: "Network Out", color: "var(--color-chart-4)" },
  ];
</script>

<div class="overflow-hidden rounded-lg border border-border bg-background">
  <svg viewBox={`0 0 ${width} ${height}`} class="h-[320px] w-full overflow-visible">
    <g stroke="var(--border)" stroke-opacity="0.6">
      {#each Array.from({ length: 5 }) as _, index}
        <line x1={plotLeft} x2={plotRight} y1={plotTop + (plotHeight / 4) * index} y2={plotTop + (plotHeight / 4) * index} />
      {/each}
    </g>

    <g class="fill-muted-foreground text-[10px]">
      {#each leftTicks as tick, index}
        <text x={plotLeft - 8} y={plotTop + plotHeight - (plotHeight / 4) * index + 3} text-anchor="end">
          {tick}%
        </text>
      {/each}
    </g>

    <g class="fill-muted-foreground text-[10px]">
      {#each rightTicks as tick, index}
        {@const value = Math.round(maxUsage * tick)}
        <text x={plotRight + 8} y={plotTop + plotHeight - (plotHeight / 4) * index + 3} text-anchor="start">
          {value >= 1024 ? `${(value / 1024).toFixed(1)}k` : `${value}`}
        </text>
      {/each}
    </g>

    {#if cpuPath}
      <path d={cpuPath} fill="none" stroke="var(--color-chart-1)" stroke-width="3" />
    {/if}
    {#if ramPath}
      <path d={ramPath} fill="none" stroke="var(--color-chart-2)" stroke-width="3" />
    {/if}
    {#if rxPath}
      <path d={rxPath} fill="none" stroke="var(--color-chart-3)" stroke-width="2" stroke-dasharray="5 4" />
    {/if}
    {#if txPath}
      <path d={txPath} fill="none" stroke="var(--color-chart-4)" stroke-width="2" stroke-dasharray="2 4" />
    {/if}

    {#each points as point, index}
      <text x={plotLeft + (plotWidth / Math.max(points.length - 1, 1)) * index} y={height - 8} text-anchor="middle" class="fill-muted-foreground text-[10px]">
        {point.time}
      </text>
    {/each}
  </svg>

  <div class="flex flex-wrap items-center gap-x-5 gap-y-2 border-t border-border px-4 py-3 text-sm text-muted-foreground">
    {#each legend as item}
      <div class="flex items-center gap-2">
        <span class="size-2 rounded-full" style={`background-color: ${item.color}`}></span>
        <span>{item.label}</span>
      </div>
    {/each}
  </div>
</div>
