<script lang="ts">
  const { class: className = "", ...rest } = $$restProps;

  export let checked = false;
  export let disabled = false;

  const toggle = () => {
    if (!disabled) checked = !checked;
  };
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  {disabled}
  class={`relative inline-flex h-6 w-11 shrink-0 items-center rounded-full border border-transparent transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 ${
    checked ? "bg-primary" : "bg-muted"
  } ${className}`}
  {...rest}
  on:click={toggle}
>
  <span
    class={`pointer-events-none block size-5 rounded-full bg-background shadow-sm ring-0 transition-transform ${
      checked ? "translate-x-5" : "translate-x-0"
    }`}
  ></span>
</button>
