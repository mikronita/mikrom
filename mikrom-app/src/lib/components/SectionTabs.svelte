<script lang="ts">
  import { cn } from "$lib/utils";

  type TabItem = {
    value: string;
    label: string;
  };

  let {
    tabs = [],
    active = $bindable(""),
    class: className = "",
    onChange,
    ...rest
  } = $props<{
    tabs?: ReadonlyArray<TabItem>;
    active?: string;
    class?: string;
    onChange?: (value: string) => void;
  }>();

  function setActive(value: string) {
    active = value;
    onChange?.(value);
  }
</script>

<div class={cn("border-b border-border", className)} {...rest}>
  <div class="grid h-auto w-full grid-cols-2 gap-0.5 sm:grid-cols-3 xl:grid-cols-6">
    {#each tabs as tab}
      <button
        type="button"
        class={`flex items-center justify-start gap-2 border-b-2 px-4 py-2.5 text-sm font-medium transition-colors sm:justify-center ${
          active === tab.value
            ? "border-primary text-foreground"
            : "border-transparent text-muted-foreground hover:text-foreground"
        }`}
        onclick={() => setActive(tab.value)}
      >
        <span class="truncate">{tab.label}</span>
      </button>
    {/each}
  </div>
</div>
