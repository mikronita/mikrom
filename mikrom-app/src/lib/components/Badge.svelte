<script lang="ts">
  const { class: classAttr = "", ...rest } = $$restProps;

  export let variant: "default" | "secondary" | "outline" | "success" | "warning" | "destructive" = "default";
  export let className = "";
  $: mergedClassName = `${className} ${classAttr}`.trim();

  const variantStyles = {
    default:
      "background-color: var(--muted); border-color: var(--border); color: var(--foreground);",
    secondary:
      "background-color: var(--muted); border-color: var(--border); color: var(--foreground);",
    outline:
      "background-color: var(--background); border-color: var(--border); color: var(--foreground);",
    success:
      "background-color: color-mix(in srgb, var(--status-online) 10%, transparent); border-color: color-mix(in srgb, var(--status-online) 30%, transparent); color: var(--status-online);",
    warning:
      "background-color: color-mix(in srgb, var(--status-warning) 10%, transparent); border-color: color-mix(in srgb, var(--status-warning) 30%, transparent); color: var(--status-warning);",
    destructive:
      "background-color: color-mix(in srgb, var(--status-offline) 10%, transparent); border-color: color-mix(in srgb, var(--status-offline) 30%, transparent); color: var(--status-offline);",
  } as const;
</script>

<span
  class={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs font-medium transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 [&_svg]:size-3.5 [&_svg]:shrink-0 ${mergedClassName}`}
  style={variantStyles[variant]}
  {...rest}
>
  <slot />
</span>
