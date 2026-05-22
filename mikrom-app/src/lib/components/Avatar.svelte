<script lang="ts">
  import { Avatar as AvatarRoot } from "./ui/avatar/index.js";
  import { cn } from "$lib/utils";

  let { class: className = "", children, ...rest } = $props();
</script>

<AvatarRoot class={cn(className)} {...rest}>
  {@render children?.()}
</AvatarRoot>
