<script lang="ts">
  import { cn } from "$lib/utils";
  const { class: className = "", ...rest } = $$restProps;
</script>

<tr
  class={cn(
    "border-b transition-colors hover:bg-muted/50 data-[state=selected]:bg-muted",
    className
  )}
  {...rest}
>
  <slot />
</tr>
