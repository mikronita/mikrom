<script lang="ts">
  import { Progress as ProgressRoot } from "./ui/progress/index.js";
  import { cn } from "$lib/utils";

  let { value = 0, max = 100, class: className = "", ...rest } = $props();
</script>

<ProgressRoot {value} {max} class={cn(className)} {...rest} />
