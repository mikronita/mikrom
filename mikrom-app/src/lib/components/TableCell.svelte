<script lang="ts">
  import { TableCell as Cell } from "./ui/table/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<Cell class={cn(className)} {...rest}>
  {@render children?.()}
</Cell>
