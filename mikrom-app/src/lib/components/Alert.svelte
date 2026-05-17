<script lang="ts">
  const { class: classAttr = "", ...rest } = $$restProps;

  export let variant: "default" | "destructive" = "default";
  export let className = "";
  $: mergedClassName = `${className} ${classAttr}`.trim();
</script>

<div
  role="alert"
  class={`relative w-full rounded-md border px-4 py-3 text-sm [&>svg+div]:translate-y-[-3px] [&>svg]:absolute [&>svg]:left-4 [&>svg]:top-4 [&>svg]:size-4 [&>svg]:shrink-0 [&>svg]:text-foreground [&>svg~*]:pl-7 ${variant === "destructive" ? "border-destructive/30 bg-destructive/10 text-destructive [&>svg]:text-destructive" : "bg-background text-foreground"} ${mergedClassName}`}
  {...rest}
>
  <slot />
</div>
