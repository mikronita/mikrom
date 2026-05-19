<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { X } from "lucide-svelte";
  import Button from "$lib/components/Button.svelte";

  export let open = false;
  export let title = "";
  export let description = "";
  export let confirmLabel = "Confirm";
  export let cancelLabel = "Cancel";
  export let confirmVariant: "default" | "destructive" = "destructive";
  export let disabled = false;

  const dispatch = createEventDispatcher<{ close: void; confirm: void }>();

  function close() {
    open = false;
    dispatch("close");
  }

  function confirm() {
    dispatch("confirm");
  }

  onMount(() => {
    const handleKeydown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && open) {
        close();
      }
    };

    window.addEventListener("keydown", handleKeydown);
    return () => window.removeEventListener("keydown", handleKeydown);
  });
</script>

{#if open}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-black/50 p-3 backdrop-blur-[1px] sm:p-0"
    role="presentation"
    on:click={(event) => event.target === event.currentTarget && close()}
  >
    <div class="w-full">
      <div class="relative mx-auto grid max-h-[calc(100vh-1.5rem)] w-full max-w-lg gap-4 overflow-y-auto rounded-lg border bg-background p-6 shadow-xl" role="alertdialog" aria-modal="true" aria-labelledby="alert-dialog-title" aria-describedby={description ? "alert-dialog-description" : undefined}>
        <div class="flex flex-col gap-1.5 text-left">
          <h2 id="alert-dialog-title" class="text-lg font-semibold leading-none tracking-tight">{title}</h2>
          {#if description}
            <p id="alert-dialog-description" class="text-sm text-muted-foreground">{description}</p>
          {/if}
        </div>

        <slot />

        <div class="flex justify-end gap-2">
          <Button variant="outline" onclick={close}>{cancelLabel}</Button>
          <Button variant={confirmVariant} onclick={confirm} disabled={disabled}>{confirmLabel}</Button>
        </div>

        <button
          type="button"
          class="absolute right-4 top-4 rounded-sm opacity-70 ring-offset-background transition-opacity hover:opacity-100 focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2"
          on:click={close}
          aria-label="Close"
        >
          <X class="size-4" />
        </button>
      </div>
    </div>
  </div>
{/if}
