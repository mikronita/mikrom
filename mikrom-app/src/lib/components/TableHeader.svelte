<script lang="ts">
  import { cn } from "$lib/utils";
  const { class: className = "", ...rest } = $$restProps;
</script>

<thead class={cn("[&_tr]:border-b", className)} {...rest}>
  <slot />
</thead>
