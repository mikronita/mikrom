<script lang="ts">
  import { ChevronDown } from "lucide-svelte";
  const { class: className = "", ...rest } = $$restProps;

  export let value: any = undefined;
  export let id: string | undefined = undefined;
  export let required = false;
  export let disabled = false;
</script>

<div class="relative w-full">
  <select
    {id}
    bind:value
    {required}
    {disabled}
    class={`flex h-9 w-full appearance-none rounded-md border border-input bg-background px-3 py-1 pr-8 text-sm shadow-none transition-colors placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 ${className}`}
    {...rest}
    on:change
  >
    <slot />
  </select>
  <div class="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-2.5">
    <ChevronDown class="size-4 opacity-50" />
  </div>
</div>
