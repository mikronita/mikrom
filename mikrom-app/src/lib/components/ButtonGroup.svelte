<script lang="ts">
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<div 
  class={cn("button-group inline-flex items-stretch", className)} 
  role="group" 
  data-slot="button-group"
  {...rest}
>
  {@render children?.()}
</div>

<style>
  :global(.button-group > *) {
    border-radius: 0;
  }

  :global(.button-group > * + *) {
    margin-left: -1px;
  }

  :global(.button-group > *:first-child) {
    border-top-left-radius: var(--radius-md);
    border-bottom-left-radius: var(--radius-md);
  }

  :global(.button-group > *:last-child) {
    border-top-right-radius: var(--radius-md);
    border-bottom-right-radius: var(--radius-md);
  }

  :global(.button-group > *:focus-visible) {
    position: relative;
    z-index: 1;
  }
</style>
