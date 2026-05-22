<script lang="ts">
  import { Table as TableRoot } from "./ui/table/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<TableRoot class={cn(className)} {...rest}>
  {@render children?.()}
</TableRoot>
