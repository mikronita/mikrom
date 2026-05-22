<script lang="ts">
  import { Skeleton as SkeletonRoot } from "./ui/skeleton/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", ...rest } = $props();
</script>

<SkeletonRoot class={cn(className)} {...rest} />
