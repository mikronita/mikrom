<script lang="ts">
  const { class: className = "", ...rest } = $$restProps;
</script>

<div class={`flex h-full w-full items-center justify-center bg-muted text-muted-foreground ${className}`} {...rest}>
  <slot />
</div>
