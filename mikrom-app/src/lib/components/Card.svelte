<script lang="ts">
  import { Card as CardRoot } from "./ui/card/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<CardRoot class={cn(className)} {...rest}>
  {@render children?.()}
</CardRoot>
