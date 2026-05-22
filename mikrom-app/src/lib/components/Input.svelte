<script lang="ts">
  import { Input } from "./ui/input/index.js";
  import { cn } from "$lib/utils";

  let {
    value = $bindable(),
    readonly = false,
    readOnly = false,
    class: className = "",
    ...rest
  }: {
    value?: any;
    readonly?: boolean;
    readOnly?: boolean;
    class?: string;
    [key: string]: any;
  } = $props();

  const isReadonly = $derived(readonly || readOnly);
</script>

<Input bind:value readonly={isReadonly} class={cn(className)} {...rest} />
