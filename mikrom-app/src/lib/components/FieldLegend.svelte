<script lang="ts">
  import { cn } from "$lib/utils";
  export let className: string = "";
</script>

<legend class={cn("text-sm font-bold uppercase tracking-wider text-muted-foreground mb-2", className)}>
  <slot />
</legend>
