<script lang="ts">
  import { toasts, dismiss } from "$lib/toast";
</script>

<div class="fixed right-3 top-3 z-[60] flex w-[calc(100vw-1.5rem)] max-w-sm flex-col gap-2 sm:w-96">
  {#each $toasts as toast (toast.id)}
    <div class={`rounded-md border px-4 py-3 shadow-lg ${toast.variant === "error" ? "border-destructive/30 bg-destructive/10 text-destructive" : toast.variant === "success" ? "border-status-online/30 bg-[color:color-mix(in_srgb,var(--status-online)_12%,white)] text-[var(--status-online)]" : "border-border bg-card text-foreground"}`}>
      <div class="flex items-start justify-between gap-3">
        <p class="text-sm">{toast.message}</p>
        <button class="text-xs text-muted-foreground hover:text-foreground" on:click={() => dismiss(toast.id)}>x</button>
      </div>
    </div>
  {/each}
</div>
