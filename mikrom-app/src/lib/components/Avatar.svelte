<script lang="ts">
  const { class: className = "", ...rest } = $$restProps;
</script>

<div class={`relative flex shrink-0 overflow-hidden ${className}`} {...rest}>
  <slot />
</div>
