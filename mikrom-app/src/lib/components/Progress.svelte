<script lang="ts">
  import { cn } from "$lib/utils";

  export let value: number | null = null;
  export let max = 100;
  export let className = "";

  const { class: classAttr = "", ...rest } = $$restProps;

  $: mergedClassName = cn(
    "relative h-2 w-full overflow-hidden rounded-full bg-muted",
    className,
    classAttr,
  );

  $: clampedValue = value === null ? null : Math.max(0, Math.min(value, max));
  $: percent = clampedValue === null ? 0 : (clampedValue / max) * 100;
</script>

<div
  class={mergedClassName}
  role="progressbar"
  aria-valuemin="0"
  aria-valuemax={max}
  aria-valuenow={clampedValue ?? undefined}
  {...rest}
>
  {#if clampedValue === null}
    <div class="progress-indeterminate absolute inset-y-0 left-0 w-1/2 rounded-full bg-primary"></div>
  {:else}
    <div class="h-full rounded-full bg-primary transition-[width] duration-300" style={`width: ${percent}%`}></div>
  {/if}
</div>

<style>
  @keyframes progress-indeterminate {
    0% {
      transform: translateX(-120%);
    }
    50% {
      transform: translateX(40%);
    }
    100% {
      transform: translateX(220%);
    }
  }

  .progress-indeterminate {
    animation: progress-indeterminate 1.4s ease-in-out infinite;
  }
</style>
