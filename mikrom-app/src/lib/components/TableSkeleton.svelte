<script lang="ts">
  import { cn } from "$lib/utils";
  import Skeleton from "$lib/components/Skeleton.svelte";

  type TableSkeletonColumn = {
    skeletonClassName: string;
    cellClassName?: string;
  };

  export let rows = 3;
  export let columns: TableSkeletonColumn[] = [];
  export let rowClassName = "border-b border-border";
  export let cellClassName = "px-4 py-4";

  const { class: classAttr = "", ...rest } = $$restProps;
</script>

<tbody class={classAttr} {...rest}>
  {#each Array.from({ length: rows }) as _}
    <tr class={rowClassName}>
      {#each columns as column}
        <td class={cn(cellClassName, column.cellClassName)}>
          <Skeleton className={column.skeletonClassName} />
        </td>
      {/each}
    </tr>
  {/each}
</tbody>
