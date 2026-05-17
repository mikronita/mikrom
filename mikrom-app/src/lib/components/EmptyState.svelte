<script lang="ts">
  const { class: className = "", ...rest } = $$restProps;
</script>

<div
  class={`flex min-w-0 flex-1 flex-col items-center justify-center gap-6 rounded-md border border-dashed border-border bg-muted/20 p-6 text-center md:p-12 ${className}`}
  {...rest}
>
  <div class="flex max-w-sm flex-col items-center gap-2 text-center">
    <slot />
  </div>
</div>
