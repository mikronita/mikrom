<script lang="ts">
  import { cn } from "$lib/utils";
  let { class: className = "", label = "", children } = $props();
</script>

<div class={cn("flex flex-col gap-6", className)}>
  {#if label}
    <h3 class="text-sm font-medium text-foreground">{label}</h3>
  {/if}
  {@render children?.()}
</div>
