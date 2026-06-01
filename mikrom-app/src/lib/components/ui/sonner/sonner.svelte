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

<style>
	:global([data-sonner-toast]) {
		transition:
			transform 260ms cubic-bezier(0.22, 1, 0.36, 1),
			opacity 180ms ease,
			height 260ms cubic-bezier(0.22, 1, 0.36, 1),
			box-shadow 180ms ease;
		transform-origin: center top;
	}

	:global([data-sonner-toast][data-removed="true"]) {
		transition:
			transform 200ms cubic-bezier(0.4, 0, 1, 1),
			opacity 160ms ease,
			height 200ms cubic-bezier(0.4, 0, 1, 1),
			box-shadow 160ms ease;
	}

	:global([data-sonner-toast][data-styled="true"]) {
		box-shadow:
			0 10px 28px color-mix(in oklab, var(--color-background) 24%, transparent),
			0 1px 0 color-mix(in oklab, var(--color-border) 58%, transparent) inset;
	}

	:global([data-sonner-toast][data-styled="true"]:hover) {
		box-shadow:
			0 14px 34px color-mix(in oklab, var(--color-background) 30%, transparent),
			0 1px 0 color-mix(in oklab, var(--color-border) 58%, transparent) inset;
	}

	:global([data-sonner-toast][data-styled="true"][data-type="warning"]) {
		background: color-mix(in oklab, var(--bg-overlay) 90%, var(--status-warning));
		border-color: var(--color-border);
		color: var(--color-foreground);
	}

	:global([data-sonner-toast][data-styled="true"][data-type="error"]) {
		background: color-mix(in oklab, var(--bg-overlay) 90%, var(--status-offline));
		border-color: var(--color-border);
		color: var(--color-foreground);
	}

	:global([data-sonner-toast][data-styled="true"][data-type="info"]) {
		background: color-mix(in oklab, var(--bg-overlay) 92%, var(--status-info));
		border-color: var(--color-border);
		color: var(--color-foreground);
	}

	:global([data-sonner-toast][data-styled="true"][data-type="warning"] :global([data-icon])),
	:global([data-sonner-toast][data-styled="true"][data-type="error"] :global([data-icon])),
	:global([data-sonner-toast][data-styled="true"][data-type="info"] :global([data-icon])) {
		color: var(--color-foreground);
	}
</style>
