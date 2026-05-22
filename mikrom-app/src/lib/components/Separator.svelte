<script lang="ts">
  import { Separator as SeparatorRoot } from "./ui/separator/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", ...rest } = $props();
</script>

<SeparatorRoot class={cn(className)} {...rest} />
