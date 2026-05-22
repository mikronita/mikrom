<script lang="ts">
  let { label = "", description = "", forId = undefined, children } = $props();
</script>

<div class="flex flex-col gap-1.5">
  {#if label}
    <label for={forId} class="text-sm font-medium">{label}</label>
  {/if}
  {@render children?.()}
  {#if description}
    <p class="text-xs text-muted-foreground">{description}</p>
  {/if}
</div>
