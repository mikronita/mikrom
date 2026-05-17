<script lang="ts">
  const { class: className = "", ...rest } = $$restProps;
</script>

<div class={`rounded-md border border-border bg-card text-card-foreground shadow-sm ${className}`} {...rest}>
  <slot />
</div>
