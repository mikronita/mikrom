<script lang="ts">
  import { CardContent as Content } from "./ui/card/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<Content class={cn(className)} {...rest}>
  {@render children?.()}
</Content>
