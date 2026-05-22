<script lang="ts">
  import { TableBody as Body } from "./ui/table/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<Body class={cn(className)} {...rest}>
  {@render children?.()}
</Body>
