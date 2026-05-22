<script lang="ts">
  import { CardHeader as Header } from "./ui/card/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<Header class={cn(className)} {...rest}>
  {@render children?.()}
</Header>
