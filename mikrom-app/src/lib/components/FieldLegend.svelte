<script lang="ts">
  import { cn } from "$lib/utils";
  let { class: className = "", children } = $props();
</script>

<legend class={cn("text-sm font-bold uppercase tracking-wider text-muted-foreground mb-2", className)}>
  {@render children?.()}
</legend>
