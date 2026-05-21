<script lang="ts">
  import { cn } from "$lib/utils";
  const { class: className = "", ...rest } = $$restProps;
</script>

<tbody class={cn("[&_tr:last-child]:border-0", className)} {...rest}>
  <slot />
</tbody>
