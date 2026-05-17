<script lang="ts">
  const { class: className = "", ...rest } = $$restProps;
</script>

<h3 class={`font-semibold leading-none tracking-tight ${className}`} {...rest}>
  <slot />
</h3>
