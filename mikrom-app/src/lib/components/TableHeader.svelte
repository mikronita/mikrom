<script lang="ts">
  import { TableHeader as Header } from "./ui/table/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<Header class={cn(className)} {...rest}>
  {@render children?.()}
</Header>
