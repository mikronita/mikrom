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
  		value?: string | string[] | undefined;
  		type?: "single" | "multiple";
  		children?: Snippet;
  		[key: string]: unknown;
  	} = $props();
</script>

{#if type === "multiple"}
	<SelectPrimitive.Root bind:open bind:value={value as string[] | undefined} {type} {...restProps}>
		{@render children?.()}
	</SelectPrimitive.Root>
{:else}
	<SelectPrimitive.Root bind:open bind:value={value as string | undefined} {type} {...restProps}>
		{@render children?.()}
	</SelectPrimitive.Root>
{/if}
