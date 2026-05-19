<script lang="ts">
  import { cn } from "$lib/utils";

  export let className = "";
  const { class: classAttr = "", ...rest } = $$restProps;

  $: mergedClassName = cn("animate-pulse rounded-md bg-muted/80", className, classAttr);
</script>

<div class={mergedClassName} {...rest}></div>
