<script lang="ts">
  import { cn } from "$lib/utils";
  import Skeleton from "$lib/components/Skeleton.svelte";

  export let showIcon = true;
  export let showBadge = true;
  export let iconClassName = "size-10 rounded-lg";
  export let titleClassName = "w-32";
  export let descriptionClassName = "w-full";
  export let footerPills: string[] = [];
  export let footerLineClassName = "w-40";
  export let compact = false;

  const { class: classAttr = "", ...rest } = $$restProps;
</script>

<div class={cn("overflow-hidden rounded-lg border bg-card shadow-sm", classAttr)} {...rest}>
  <div class={compact ? "p-4" : "p-5"}>
    <div class="flex items-start gap-4">
      {#if showIcon}
        <Skeleton className={cn("shrink-0", iconClassName)} />
      {/if}
      <div class="flex min-w-0 flex-1 flex-col gap-2">
        <Skeleton className={cn("h-5", titleClassName)} />
        <Skeleton className={cn("h-4", descriptionClassName)} />
      </div>
      {#if showBadge}
        <Skeleton className="h-6 w-14 rounded-full" />
      {/if}
    </div>

    {#if footerPills.length > 0 || footerLineClassName}
      <div class="mt-4 flex flex-col gap-3">
        {#if footerLineClassName}
          <Skeleton className={cn("h-4", footerLineClassName)} />
        {/if}
        {#if footerPills.length > 0}
          <div class="flex flex-wrap gap-2">
            {#each footerPills as pill}
              <Skeleton className={cn("h-6 rounded-full", pill)} />
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>
