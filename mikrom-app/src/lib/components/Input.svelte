<script lang="ts">
  const { class: className = "", ...rest } = $$restProps;

  export let value: string | number = "";
  export let type: string = "text";
  export let placeholder: string | undefined = undefined;
  export let disabled = false;
  export let id: string | undefined = undefined;
  export let required = false;
  export let readOnly = false;
  export let min: number | string | undefined = undefined;
  export let max: number | string | undefined = undefined;
</script>

<input
  bind:value
  {type}
  {placeholder}
  {disabled}
  {id}
  {required}
  readonly={readOnly}
  {min}
  {max}
  class={`flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm shadow-none transition-colors file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 ${className}`}
  {...rest}
/>
