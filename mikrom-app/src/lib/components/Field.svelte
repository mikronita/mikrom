<script lang="ts">
  export let label = "";
  export let description = "";
  export let forId: string | undefined = undefined;
</script>

<div class="flex flex-col gap-1.5">
  {#if label}
    <label for={forId} class="text-sm font-medium">{label}</label>
  {/if}
  <slot />
  {#if description}
    <p class="text-xs text-muted-foreground">{description}</p>
  {/if}
</div>
