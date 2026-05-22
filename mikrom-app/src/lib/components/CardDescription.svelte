<script lang="ts">
  import { CardDescription as Description } from "./ui/card/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<Description class={cn(className)} {...rest}>
  {@render children?.()}
</Description>
