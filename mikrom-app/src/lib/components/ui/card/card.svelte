<script lang="ts">
	import type { HTMLAttributes } from "svelte/elements";
	import { cn, type WithElementRef } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		children,
		size = "default",
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLDivElement>> & { size?: "default" | "sm" } = $props();
</script>

<div
	bind:this={ref}
	data-slot="card"
	data-size={size}
	class={cn("group/card flex flex-col gap-4 overflow-hidden rounded-2xl border border-border/70 bg-card text-card-foreground text-sm shadow-xs transition-shadow duration-200 hover:shadow-sm data-[size=sm]:gap-3 data-[size=sm]:rounded-xl", className)}
	{...restProps}
>
	{@render children?.()}
</div>
