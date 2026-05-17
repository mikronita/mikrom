<script lang="ts">
  const { class: className = "", ...rest } = $$restProps;
</script>

<p class={`text-sm text-muted-foreground ${className}`} {...rest}>
  <slot />
</p>
