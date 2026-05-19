<script lang="ts">
  import { cn } from "$lib/utils";

  const { class: className = "", ...rest } = $$restProps;
</script>

<div class={cn("p-5 pt-0 sm:p-6 sm:pt-0", className)} {...rest}>
  <slot />
</div>
