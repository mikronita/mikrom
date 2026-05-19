<script lang="ts">
  import { cn } from "$lib/utils";

  const { class: className = "", ...rest } = $$restProps;
</script>

<div class={cn("rounded-lg border bg-card text-card-foreground shadow-sm", className)} {...rest}>
  <slot />
</div>
