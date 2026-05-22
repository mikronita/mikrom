<script lang="ts">
  import { TableRow as Row } from "./ui/table/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<Row class={cn(className)} {...rest}>
  {@render children?.()}
</Row>
