<script lang="ts">
  const { class: className = "", ...rest } = $$restProps;
</script>

<div class={`p-5 sm:p-6 pt-0 ${className}`} {...rest}>
  <slot />
</div>
