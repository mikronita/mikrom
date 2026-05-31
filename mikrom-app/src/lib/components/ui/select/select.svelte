<script lang="ts">
	import { Select as SelectPrimitive } from "bits-ui";
	import type { Snippet } from "svelte";

	let {
		open = $bindable(false),
		value = $bindable(""),
		children,
		...restProps
  	}: {
  		open?: boolean;
  		value?: string | undefined;
  		children?: Snippet;
  		[key: string]: unknown;
  	} = $props();
</script>

<SelectPrimitive.Root bind:open bind:value type="single" {...restProps}>
	{@render children?.()}
</SelectPrimitive.Root>
