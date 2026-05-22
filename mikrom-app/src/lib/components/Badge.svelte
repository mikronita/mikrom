<script lang="ts">
  import { Badge as BadgeRoot } from "./ui/badge/index.js";
  import { cn } from "$lib/utils";

  let {
    variant = "default",
    class: className = "",
    children,
    ...rest
  }: {
    variant?: any;
    class?: string;
    children?: any;
    [key: string]: any;
  } = $props();
</script>

<BadgeRoot {variant} class={cn(className)} {...rest}>
  {@render children?.()}
</BadgeRoot>
