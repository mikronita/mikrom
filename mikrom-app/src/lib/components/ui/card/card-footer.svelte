<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	let {
		ref = $bindable(null),
		class: className,
		children,
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLDivElement>> = $props();
</script>

<div
	bind:this={ref}
	data-slot="card-footer"
	class={cn("rounded-b-2xl px-6 py-4 group-data-[size=sm]/card:px-4 group-data-[size=sm]/card:py-3 [.border-t]:pt-5 group-data-[size=sm]/card:[.border-t]:pt-3 flex items-center", className)}
	{...restProps}
>
	{@render children?.()}
</div>
