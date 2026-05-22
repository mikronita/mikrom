<script lang="ts">
  import { AvatarFallback as Fallback } from "./ui/avatar/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<Fallback class={cn(className)} {...rest}>
  {@render children?.()}
</Fallback>
