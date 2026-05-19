<script lang="ts">
  import { cn } from "$lib/utils";

  const { class: className = "", ...rest } = $$restProps;
</script>

<div class={cn("flex flex-col gap-1.5 p-5 sm:p-6", className)} {...rest}>
  <slot />
</div>
