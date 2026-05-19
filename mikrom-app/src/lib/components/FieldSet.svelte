<script lang="ts">
  import { cn } from "$lib/utils";
  export let className: string = "";
</script>

<fieldset class={cn("space-y-4", className)}>
  <slot />
</fieldset>
