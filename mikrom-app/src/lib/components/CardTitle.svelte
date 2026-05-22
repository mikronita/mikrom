<script lang="ts">
  import { CardTitle as Title } from "./ui/card/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<Title class={cn(className)} {...rest}>
  {@render children?.()}
</Title>
