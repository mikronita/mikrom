<script lang="ts">
  import {
    Badge,
    Button,
    Card,
    CardContent,
    CardDescription,
    CardFooter,
    CardHeader,
    CardTitle,
    EmptyState,
    Skeleton,
  } from "$lib/components";
  import type { BillingProduct, BillingSummary } from "$lib/api";
  import { getBillingStatusConfig } from "$lib/domain/billing";

  let {
    billing = null,
    products = [],
    productsLoading = false,
    productsRefreshing = false,
    lastSyncedAt = null,
    loading = false,
    actionLoading = false,
    selectionLoading = false,
    canManageBilling = true,
    error = "",
    onChangePlan = () => {},
    onCheckoutProductChange = () => {},
    onRefreshProducts = () => {},
    onManageBilling = () => {},
  } = $props<{
    billing?: BillingSummary | null;
    products?: BillingProduct[];
    productsLoading?: boolean;
    productsRefreshing?: boolean;
    lastSyncedAt?: string | null;
    loading?: boolean;
    actionLoading?: boolean;
    selectionLoading?: boolean;
    canManageBilling?: boolean;
    error?: string;
    onChangePlan?: (productId?: string | null) => void;
    onCheckoutProductChange?: (productId?: string | null) => void | Promise<void>;
    onRefreshProducts?: () => void | Promise<void>;
    onManageBilling?: () => void;
  }>();

  function formatMoney(amountCents: number | null, currency: string | null) {
    if (amountCents === null || !currency) return "Included in your plan";
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency,
      maximumFractionDigits: 2,
    }).format(amountCents / 100);
  }

  function isCheckoutAction(handler: "changePlan" | "manageBilling") {
    return handler === "changePlan";
  }

  function isActionDisabled(handler: "changePlan" | "manageBilling") {
    return (
      actionLoading ||
      loading ||
      (isCheckoutAction(handler) && (selectionLoading || !selectedCheckoutProductId))
    );
  }

  function handlePrimaryAction() {
    if (currentBilling.primaryHandler === "manageBilling") {
      onManageBilling();
      return;
    }

    onChangePlan(selectedCheckoutProductId || null);
  }

  function handleSecondaryAction() {
    if (currentBilling.secondaryHandler === "manageBilling") {
      onManageBilling();
      return;
    }

    onChangePlan(selectedCheckoutProductId || null);
  }

  function handleRefreshProducts() {
    void onRefreshProducts();
  }

  function formatSyncedAt(value: string | null) {
    if (!value) return "Not synced yet";
    return new Intl.DateTimeFormat("en-US", {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date(value));
  }

  function formatDate(value: string | null) {
    if (!value) return "Not scheduled";
    return new Intl.DateTimeFormat("en-US", {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date(value));
  }

  function resolveProductName(productId: string | null | undefined): string | null {
    if (!productId) return null;
    return products.find((p: BillingProduct) => p.id === productId)?.name ?? null;
  }

  let currentBilling = $derived(getBillingStatusConfig(billing?.status));
  let isTestMode = $derived(billing?.is_test_mode ?? false);
  let defaultCheckoutProduct: BillingProduct | null = $derived(
    products.find((product: BillingProduct) => product.id === billing?.default_checkout_product_id) || null,
  );
  let selectedCheckoutProductId = $state("");
  let checkoutSelectionSourceKey = $derived(
    [
      billing?.selected_checkout_product_id || "",
      billing?.default_checkout_product_id || "",
      products.map((product: BillingProduct) => product.id).join(","),
    ].join("|"),
  );
  let lastCheckoutSelectionSourceKey = $state("");

  function resolveCheckoutProductId() {
    if (selectedCheckoutProductId) {
      const currentSelection = products.find(
        (product: BillingProduct) => product.id === selectedCheckoutProductId,
      );
      if (currentSelection) {
        return currentSelection.id;
      }
    }

    const persistedSelection = billing?.selected_checkout_product_id
      ? products.find((product: BillingProduct) => product.id === billing.selected_checkout_product_id)
      : null;
    if (persistedSelection) {
      return persistedSelection.id;
    }

    const defaultCheckoutSelection = products.find(
      (product: BillingProduct) => product.id === billing?.default_checkout_product_id,
    );
    if (defaultCheckoutSelection) {
      return defaultCheckoutSelection.id;
    }

    return products[0]?.id || "";
  }

  $effect(() => {
    if (checkoutSelectionSourceKey === lastCheckoutSelectionSourceKey) return;
    lastCheckoutSelectionSourceKey = checkoutSelectionSourceKey;
    selectedCheckoutProductId = resolveCheckoutProductId();
  });

  async function handleCheckoutProductChange(nextProductId: string | null) {
    if (!nextProductId) {
      selectedCheckoutProductId = "";
      if (!canManageBilling) return;
      await onCheckoutProductChange(null);
      return;
    }

    selectedCheckoutProductId = nextProductId;
    if (!canManageBilling) return;
    await onCheckoutProductChange(nextProductId);
  }
</script>

<div class="flex flex-col gap-4">
  <Card>
    <CardHeader>
      <div class="flex items-start justify-between gap-3">
        <div>
          <CardTitle>Plans</CardTitle>
        </div>
        <Button
          variant="outline"
          size="sm"
          onclick={handleRefreshProducts}
          disabled={productsLoading || productsRefreshing || !canManageBilling}
        >
          {productsRefreshing ? "Refreshing..." : "Refresh"}
        </Button>
      </div>
    </CardHeader>
    <CardContent class="pt-0">
      <p class="mb-2 text-xs text-muted-foreground">
        {formatSyncedAt(lastSyncedAt)}
        {#if !canManageBilling && !isTestMode}
          (admin only)
        {:else if isTestMode} (sandbox)
        {/if}
      </p>
      {#if productsLoading}
        <div class="grid gap-3 sm:grid-cols-2">
          <Skeleton class="h-16 w-full rounded-2xl" />
          <Skeleton class="h-16 w-full rounded-2xl" />
        </div>
      {:else if products.length > 0}
        {#if !canManageBilling && !isTestMode}
          <p class="mb-2 text-xs text-muted-foreground">
            Only tenant admins can save the default checkout product or refresh the catalog.
          </p>
        {:else if isTestMode}
          <p class="mb-2 text-xs text-muted-foreground">
            Sandbox checkout is active. You can pick a plan and buy it without saving the selection.
          </p>
        {/if}

        <div class="grid gap-3 sm:grid-cols-2">
          {#each products as product}
            {@const isSelected = product.id === selectedCheckoutProductId}
            <button
              aria-label={product.name}
              class="flex items-center justify-between gap-4 rounded-2xl border p-4 text-left transition-all cursor-pointer
                {isSelected ? 'border-primary ring-1 ring-primary bg-primary/5' : 'border-border/70 bg-background/70 hover:border-border'}
                {(actionLoading || selectionLoading) ? 'opacity-50 pointer-events-none' : ''}"
              onclick={() => void handleCheckoutProductChange(product.id)}
              disabled={actionLoading || selectionLoading}
            >
              <div class="min-w-0">
                <div class="flex items-center gap-2">
                  <p class="text-sm font-medium">{product.name}</p>
                  {#if product.is_default_checkout_product}
                    <Badge variant="secondary">Default</Badge>
                  {/if}
                  {#if product.is_archived}
                    <Badge variant="outline">Archived</Badge>
                  {/if}
                </div>
                {#if product.description}
                  <p class="text-xs text-muted-foreground">{product.description}</p>
                {/if}
              </div>
              <div class="shrink-0 text-right text-sm">
                <p class="font-medium">
                  {product.price_amount_cents === null || !product.currency
                    ? "Free"
                    : new Intl.NumberFormat("en-US", {
                        style: "currency",
                        currency: product.currency,
                        maximumFractionDigits: 2,
                      }).format(product.price_amount_cents / 100)}
                </p>
                <p class="text-xs text-muted-foreground">
                  {product.recurring_interval ? `Billed ${product.recurring_interval}` : "One-time"}
                </p>
              </div>
            </button>
          {/each}
        </div>
      {:else if !productsLoading}
        <EmptyState class="min-h-[12rem]">
          <p class="text-sm font-medium">No products found</p>
          <p class="text-sm text-muted-foreground">
            Configure products in Polar and make sure the API token can read the catalog.
          </p>
        </EmptyState>
      {/if}
    </CardContent>
  </Card>

  <Card>
    <CardHeader class="flex flex-row items-start justify-between gap-4 border-b bg-muted/30">
      <div class="flex flex-col gap-1">
        {#if loading}
          <Skeleton class="h-8 w-48 rounded-md" />
          <Skeleton class="h-4 w-28 rounded-md" />
        {:else}
          <CardTitle class="text-2xl">{billing?.plan_name || "No active subscription"}</CardTitle>
          <p class="text-sm text-muted-foreground">{formatMoney(billing?.amount_cents ?? null, billing?.currency ?? null)}</p>
        {/if}
      </div>
      <div class="flex shrink-0 flex-wrap gap-2">
        <Badge variant="outline" class={currentBilling.tone}>
          {currentBilling.label}
        </Badge>
        {#if isTestMode}
          <Badge variant="secondary">Test mode</Badge>
        {/if}
      </div>
    </CardHeader>

    <CardContent class="pt-6">
      {#if error}
        <div class="rounded-2xl border border-destructive/20 bg-destructive/5 px-4 py-3 text-sm text-destructive">
          {error}
        </div>
      {/if}

      {#if !loading && currentBilling.message}
        <div class="rounded-2xl border border-amber-500/20 bg-amber-500/5 px-4 py-3 text-sm text-amber-700">
          {currentBilling.message}
        </div>
      {/if}

      {#if loading}
        <Skeleton class="h-5 w-32 rounded-md" />
      {:else}
        <div class="text-sm">
          <span class="text-muted-foreground">Next renewal: </span>
          <span class="font-medium">{formatDate(billing?.current_period_end ?? null)}</span>
        </div>
        <div class="mt-2 flex flex-wrap items-baseline gap-x-4 gap-y-1 text-xs text-muted-foreground">
          <span><span class="font-medium text-foreground">Subscription:</span> {billing?.polar_subscription_id || "None"}</span>
          <span aria-label="separator">·</span>
          <span><span class="font-medium text-foreground">Product:</span> {resolveProductName(billing?.polar_product_id) || resolveProductName(billing?.default_checkout_product_id) || billing?.polar_product_id || billing?.default_checkout_product_id || "Not configured"}</span>
        </div>
      {/if}
    </CardContent>

    <CardFooter class="flex flex-wrap items-center justify-between gap-3 border-t bg-muted/30">
      <div class="flex flex-wrap gap-2">
        <Button
          size="sm"
          onclick={handlePrimaryAction}
          disabled={isActionDisabled(currentBilling.primaryHandler)}
        >
          {currentBilling.primaryLabel}
        </Button>
        <Button
          variant="outline"
          size="sm"
          onclick={handleSecondaryAction}
          disabled={isActionDisabled(currentBilling.secondaryHandler)}
        >
          {currentBilling.secondaryLabel}
        </Button>
      </div>
      {#if billing?.has_billing_record}
        <p class="text-xs text-muted-foreground">
          Payments are managed by Polar. Customer ID: {billing.customer_external_id}
        </p>
      {/if}
    </CardFooter>
  </Card>
</div>
