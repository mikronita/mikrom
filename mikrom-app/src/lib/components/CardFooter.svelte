<script lang="ts">
  import { CardFooter as Footer } from "./ui/card/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<Footer class={cn(className)} {...rest}>
  {@render children?.()}
</Footer>
