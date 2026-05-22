<script lang="ts">
  import { Switch as SwitchRoot } from "./ui/switch/index.js";
  import { cn } from "$lib/utils";

  let { checked = $bindable(false), class: className = "", ...rest } = $props();
</script>

<SwitchRoot bind:checked class={cn(className)} {...rest} />
