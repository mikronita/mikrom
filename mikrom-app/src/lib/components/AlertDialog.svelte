<script lang="ts">
  import * as AlertDialog from "./ui/alert-dialog/index.js";


  let {
    open = $bindable(false),
    title = "",
    description = "",
    cancelText = "Cancel",
    confirmLabel = "Confirm", // Accept confirmLabel for legacy compatibility
    confirmVariant = "default", // Accept confirmVariant
    actionText = "",
    variant = "",
    loading = false,
    onaction = undefined,
    onclose = undefined,
    onconfirm = undefined, // Accept onconfirm
    ...rest
  } = $props();

  const finalActionText = $derived(actionText || confirmLabel);
  const finalVariant = $derived(variant || confirmVariant);

  function handleAction() {
    onaction?.();
    onconfirm?.();
  }

  function handleClose() {
    open = false;
    onclose?.();
  }

  function handleOpenChange(isOpen: boolean) {
    open = isOpen;
    if (!isOpen) {
      onclose?.();
    }
  }
</script>

<AlertDialog.Root {open} onOpenChange={handleOpenChange} {...rest}>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>{title}</AlertDialog.Title>
      {#if description}
        <AlertDialog.Description>{description}</AlertDialog.Description>
      {/if}
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Cancel onclick={handleClose}>{cancelText}</AlertDialog.Cancel>
      <AlertDialog.Action
        variant={finalVariant === "destructive" ? "destructive" : "default"}
        onclick={handleAction}
        disabled={loading}
        class={finalVariant === "destructive" ? "bg-destructive text-destructive-foreground hover:bg-destructive/90" : ""}
      >
        {finalActionText}
      </AlertDialog.Action>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
