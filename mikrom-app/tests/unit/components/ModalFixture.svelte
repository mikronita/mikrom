<script lang="ts">
  let {
    open = $bindable(false),
    children,
    ...rest
  } = $props();
</script>

{#if open}
  <div data-testid="modal" {...rest}>
    {@render children?.()}
  </div>
{/if}
