<script lang="ts">
	import { Toaster as Sonner, type ToasterProps as SonnerProps } from "svelte-sonner";
	import { mode } from "mode-watcher";
	import { Loader2 as Loader2Icon, CircleCheck as CircleCheckIcon, OctagonX as OctagonXIcon, Info as InfoIcon, TriangleAlert as TriangleAlertIcon } from "lucide-svelte";

	let { ...restProps }: SonnerProps = $props();
</script>

<Sonner
	theme={mode.current}
	class="toaster group"
	style="--normal-bg: var(--bg-overlay); --normal-bg-hover: color-mix(in oklab, var(--bg-overlay) 90%, var(--background)); --normal-text: var(--color-card-foreground); --normal-border: var(--color-border); --border-radius: 0.95rem;"
	{...restProps}
>
	{#snippet loadingIcon()}
		<Loader2Icon class="size-4 animate-spin" />
	{/snippet}
	{#snippet successIcon()}
		<CircleCheckIcon class="size-4" />
	{/snippet}
	{#snippet errorIcon()}
		<OctagonXIcon class="size-4" />
	{/snippet}
	{#snippet infoIcon()}
		<InfoIcon class="size-4" />
	{/snippet}
	{#snippet warningIcon()}
		<TriangleAlertIcon class="size-4" />
	{/snippet}
</Sonner>
