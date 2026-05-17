<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { X } from "lucide-svelte";

  export let open = false;
  export let title = "";
  export let description = "";
  export let width = "max-w-lg";

  const dispatch = createEventDispatcher<{ close: void }>();

  function close() {
    open = false;
    dispatch("close");
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
  <div class="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-black/50 p-3 backdrop-blur-[1px] sm:p-0" role="presentation" on:click={(event) => event.target === event.currentTarget && close()}>
    <div class="w-full">
      <div class={`relative mx-auto grid max-h-[calc(100vh-1.5rem)] gap-4 overflow-y-auto rounded-md border border-border bg-background p-6 shadow-xl ${width}`} role="dialog" aria-modal="true" aria-label={title}>
        <div class="flex flex-col space-y-1.5 text-center sm:text-left">
          <h2 class="text-lg font-semibold leading-none tracking-tight">{title}</h2>
          {#if description}
            <p class="text-sm text-muted-foreground">{description}</p>
          {/if}
        </div>
        <slot />
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
