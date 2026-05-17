<script lang="ts">
  export let value = "";
  export let placeholder: string | undefined = undefined;
  export let rows = 4;
  export let disabled = false;
</script>

<textarea
  bind:value
  {placeholder}
  {rows}
  {disabled}
  class="flex min-h-[60px] w-full rounded-md border border-input bg-transparent px-3 py-2 text-base shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 md:text-sm"
></textarea>
