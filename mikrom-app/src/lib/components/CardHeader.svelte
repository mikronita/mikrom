<script lang="ts">
  const { class: className = "", ...rest } = $$restProps;
</script>

<div class={`flex flex-col space-y-1.5 p-5 sm:p-6 ${className}`} {...rest}>
  <slot />
</div>
