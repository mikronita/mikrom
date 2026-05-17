<script lang="ts">
  const { class: classAttr = "", ...rest } = $$restProps;

  export let variant: "default" | "outline" | "ghost" | "destructive" | "secondary" | "link" = "default";
  export let size: "sm" | "md" | "lg" | "icon" = "md";
  export let type: "button" | "submit" | "reset" = "button";
  export let disabled = false;
  export let href: string | undefined = undefined;
  export let target: string | undefined = undefined;
  export let rel: string | undefined = undefined;
  export let onclick: ((event: MouseEvent) => void) | undefined = undefined;
  export let className = "";
  $: mergedClassName = `${className} ${classAttr}`.trim();

  const sizeClasses = {
    sm: "h-8 rounded-md px-3 text-xs",
    md: "h-9 px-4 py-2 text-sm",
    lg: "h-10 rounded-md px-4",
    icon: "h-9 w-9 px-0",
  } as const;

  const variantClasses = {
    default: "border border-transparent bg-primary text-primary-foreground hover:bg-primary/90",
    outline: "border-border bg-background text-foreground hover:bg-muted hover:text-foreground",
    ghost: "border border-transparent bg-transparent text-foreground hover:bg-muted hover:text-foreground",
    destructive:
      "border border-destructive/30 bg-destructive/10 text-destructive hover:bg-destructive/15 hover:text-destructive",
    secondary: "border border-border bg-secondary text-secondary-foreground hover:bg-muted",
    link: "border border-transparent bg-transparent px-0 text-primary underline-offset-4 hover:underline",
  } as const;
</script>

{#if href}
  <a
    class={`inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md border border-transparent font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0 ${sizeClasses[size]} ${variantClasses[variant]} ${mergedClassName}`}
    target={target}
    rel={rel}
    href={href}
    {...rest}
    on:click={onclick}
  >
    <slot />
  </a>
{:else}
  <button
    {type}
    {disabled}
    class={`inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md border border-transparent font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0 ${sizeClasses[size]} ${variantClasses[variant]} ${mergedClassName}`}
    {...rest}
    on:click={onclick}
  >
    <slot />
  </button>
{/if}
