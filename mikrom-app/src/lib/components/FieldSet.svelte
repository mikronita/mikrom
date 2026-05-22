<script lang="ts">
  import { cn } from "$lib/utils";
  let { class: className = "", children } = $props();
</script>

<div class={cn("space-y-4", className)}>
  {@render children?.()}
</div>
