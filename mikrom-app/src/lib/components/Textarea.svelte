<script lang="ts">
  import { Textarea as TextareaRoot } from "./ui/textarea/index.js";
  import { cn } from "$lib/utils";

  let { value = $bindable(), class: className = "", ...rest } = $props();
</script>

<TextareaRoot bind:value class={cn(className)} {...rest} />
