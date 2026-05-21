<script lang="ts">
  import { cn } from "$lib/utils";
  const { class: className = "", ...rest } = $$restProps;
</script>

<td class={cn("p-4 align-middle [&:has([role=checkbox])]:pr-0", className)} {...rest}>
  <slot />
</td>
