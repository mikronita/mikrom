<script lang="ts">
  import { cn } from "$lib/utils";

  const { class: className = "", ...rest } = $$restProps;
</script>

<div class={cn("button-group inline-flex items-stretch", className)} role="group" {...rest}>
  <slot />
</div>

<style>
  :global(.button-group > *) {
    border-radius: 0;
  }

  :global(.button-group > * + *) {
    margin-left: -1px;
  }

  :global(.button-group > *:first-child) {
    border-top-left-radius: var(--radius);
    border-bottom-left-radius: var(--radius);
  }

  :global(.button-group > *:last-child) {
    border-top-right-radius: var(--radius);
    border-bottom-right-radius: var(--radius);
  }

  :global(.button-group > *:focus-visible) {
    position: relative;
    z-index: 1;
  }
</style>
