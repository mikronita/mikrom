<script lang="ts">
  import { cn } from "$lib/utils";
  import { Skeleton } from "$lib/components";

  let {
    showIcon = true,
    showBadge = true,
    iconClassName = "size-10 rounded-md",
    titleClassName = "w-32",
    descriptionClassName = "w-full",
    footerPills = [],
    footerLineClassName = "w-40",
    compact = false,
    class: classAttr = "",
    ...rest
  } = $props();
</script>

<div class={cn("overflow-hidden rounded-lg border bg-card shadow-xs", classAttr)} {...rest}>
  <div class={compact ? "p-3" : "p-5"}>
    <div class="flex items-start gap-4">
      {#if showIcon}
        <Skeleton class={cn("shrink-0", iconClassName)} />
      {/if}
      <div class="flex min-w-0 flex-1 flex-col gap-2">
        <Skeleton class={cn("h-5", titleClassName)} />
        <Skeleton class={cn("h-4", descriptionClassName)} />
      </div>
      {#if showBadge}
        <Skeleton class="h-6 w-14 rounded-full" />
      {/if}
    </div>

    {#if footerPills.length > 0 || footerLineClassName}
      <div class="mt-4 flex flex-col gap-3">
        {#if footerLineClassName}
          <Skeleton class={cn("h-4", footerLineClassName)} />
        {/if}
        {#if footerPills.length > 0}
          <div class="flex flex-wrap gap-2">
            {#each footerPills as pill}
              <Skeleton class={cn("h-6 rounded-full", pill)} />
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>
