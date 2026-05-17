<script lang="ts">
  const { class: className = "", ...rest } = $$restProps;
</script>

<div class={`flex items-center p-5 pt-0 ${className}`} {...rest}>
  <slot />
</div>
