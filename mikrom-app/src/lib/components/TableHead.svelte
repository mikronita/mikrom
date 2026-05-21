<script lang="ts">
  import { cn } from "$lib/utils";
  const { class: className = "", ...rest } = $$restProps;
</script>

<th
  class={cn(
    "h-12 px-4 text-left align-middle font-medium text-muted-foreground [&:has([role=checkbox])]:pr-0",
    className
  )}
  {...rest}
>
  <slot />
</th>
