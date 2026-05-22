<script lang="ts">
  import { cn } from "$lib/utils";
  import { Skeleton } from "$lib/components";

  type TableSkeletonColumn = {
    skeletonClassName: string;
    cellClassName?: string;
  };

  let {
    rows = 3,
    cols = 0,
    columns = [],
    rowClassName = "border-b border-border",
    cellClassName = "px-4 py-4",
    class: classAttr = "",
    ...rest
  } = $props<{
    rows?: number;
    cols?: number;
    columns?: TableSkeletonColumn[];
    rowClassName?: string;
    cellClassName?: string;
    class?: string;
  }>();

  const finalColumns = $derived(
    columns.length > 0 
      ? columns 
      : Array.from({ length: cols || 1 }).map(() => ({ skeletonClassName: "h-4 w-full" }))
  );
</script>

<tbody class={classAttr} {...rest}>
  {#each Array.from({ length: rows }) as _}
    <tr class={rowClassName}>
      {#each finalColumns as column}
        <td class={cn(cellClassName, column.cellClassName)}>
          <Skeleton class={column.skeletonClassName} />
        </td>
      {/each}
    </tr>
  {/each}
</tbody>
