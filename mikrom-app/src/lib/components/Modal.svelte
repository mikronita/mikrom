<script lang="ts">
  import * as Dialog from "./ui/dialog/index.js";
  import { cn } from "$lib/utils";

  let {
    open = $bindable(false),
    title = "",
    description = "",
    width = "max-w-lg",
    onclose = undefined,
    children,
    ...rest
  } = $props();

  function handleOpenChange(isOpen: boolean) {
    open = isOpen;
    if (!isOpen) {
      onclose?.();
    }
  }
</script>

<Dialog.Root {open} onOpenChange={handleOpenChange} {...rest}>
  <Dialog.Content class={cn(width)}>
    <Dialog.Header>
      <Dialog.Title>{title}</Dialog.Title>
      {#if description}
        <Dialog.Description>{description}</Dialog.Description>
      {/if}
    </Dialog.Header>
    {@render children?.()}
  </Dialog.Content>
</Dialog.Root>
