<script lang="ts">
	import { Select as SelectPrimitive } from "bits-ui";
	import type { Snippet } from "svelte";

	let {
		open = $bindable(false),
		value = $bindable(),
		type = "single",
		children,
		...restProps
  	}: {
  		open?: boolean;
  		value?: any;
  		type?: "single" | "multiple";
  		children?: Snippet;
  		[key: string]: unknown;
  	} = $props();
</script>

<SelectPrimitive.Root bind:open bind:value {type} {...restProps}>
	{@render children?.()}
</SelectPrimitive.Root>
