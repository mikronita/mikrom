<script lang="ts">
  import { Badge as BadgeRoot } from "./ui/badge/index.js";
  import { cn } from "$lib/utils";

  let {
    variant = "default",
    class: className = "",
    children,
    ...rest
  }: {
    variant?: string;
    class?: string;
    children?: import("svelte").Snippet;
    [key: string]: unknown;
  } = $props();
</script>

<BadgeRoot {variant} class={cn(className)} {...rest}>
  {@render children?.()}
</BadgeRoot>
