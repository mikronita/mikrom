<script lang="ts">
  import { TableHead as Head } from "./ui/table/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<Head class={cn(className)} {...rest}>
  {@render children?.()}
</Head>
