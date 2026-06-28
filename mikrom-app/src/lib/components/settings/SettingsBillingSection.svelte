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
    Field,
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
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
      (isCheckoutAction(handler) && (!canManageBilling || selectionLoading || !selectedCheckoutProductId))
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

  let currentBilling = $derived(getBillingStatusConfig(billing?.status));
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
    if (!canManageBilling) return;
    selectedCheckoutProductId = nextProductId || "";
    await onCheckoutProductChange(nextProductId);
  }
</script>

<div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(20rem,0.85fr)]">
  <Card class="overflow-hidden">
    <CardHeader class="border-b bg-muted/30">
      <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div class="flex flex-col gap-2">
          <CardDescription>Current plan</CardDescription>
          {#if loading}
            <Skeleton class="h-8 w-48 rounded-md" />
            <Skeleton class="h-4 w-28 rounded-md" />
          {:else}
            <CardTitle class="text-2xl">{billing?.plan_name || "No active subscription"}</CardTitle>
            <p class="text-sm text-muted-foreground">{formatMoney(billing?.amount_cents ?? null, billing?.currency ?? null)}</p>
          {/if}
        </div>
        <Badge variant="outline" class={currentBilling.tone}>
          {currentBilling.label}
        </Badge>
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

      <div class="grid gap-3 sm:grid-cols-3">
        <div class="rounded-2xl border border-border/70 bg-background/70 p-5">
          {#if loading}
            <Skeleton class="mb-2 h-8 w-16 rounded-md" />
            <Skeleton class="h-4 w-24 rounded-md" />
          {:else}
            <p class="text-2xl font-semibold">{billing?.amount_cents === null ? "Free" : formatMoney(billing?.amount_cents ?? null, billing?.currency ?? null)}</p>
            <p class="text-sm text-muted-foreground">Billing cadence</p>
          {/if}
        </div>
        <div class="rounded-2xl border border-border/70 bg-background/70 p-5">
          {#if loading}
            <Skeleton class="mb-2 h-8 w-16 rounded-md" />
            <Skeleton class="h-4 w-24 rounded-md" />
          {:else}
            <p class="text-2xl font-semibold">{formatDate(billing?.current_period_end ?? null)}</p>
            <p class="text-sm text-muted-foreground">Next renewal</p>
          {/if}
        </div>
        <div class="rounded-2xl border border-border/70 bg-background/70 p-5">
          {#if loading}
            <Skeleton class="mb-2 h-8 w-16 rounded-md" />
            <Skeleton class="h-4 w-24 rounded-md" />
          {:else}
            <p class="text-2xl font-semibold">{billing?.cancel_at_period_end ? "Yes" : "No"}</p>
            <p class="text-sm text-muted-foreground">Cancel at period end</p>
          {/if}
        </div>
      </div>
    </CardContent>

    <CardFooter class="flex flex-wrap gap-2">
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
      {#if billing?.has_billing_record}
        <p class="w-full text-xs text-muted-foreground">
          Billing is managed by Polar. Customer ID: {billing.customer_external_id}
        </p>
      {/if}
    </CardFooter>
  </Card>

  <div class="flex flex-col gap-4">
    <Card>
      <CardHeader>
        <CardTitle>Billing details</CardTitle>
        <CardDescription>Polar handles payments, invoicing and card updates.</CardDescription>
      </CardHeader>
      <CardContent class="grid gap-4">
        <div class="rounded-2xl border border-border/70 bg-background/70 p-4">
          <p class="text-xs uppercase tracking-[0.2em] text-muted-foreground">Subscription ID</p>
          <p class="mt-2 truncate text-sm font-medium">{billing?.polar_subscription_id || "No subscription yet"}</p>
        </div>
        <div class="rounded-2xl border border-border/70 bg-background/70 p-4">
          <p class="text-xs uppercase tracking-[0.2em] text-muted-foreground">Product</p>
          <p class="mt-2 truncate text-sm font-medium">{billing?.polar_product_id || billing?.default_checkout_product_id || "Not configured"}</p>
          {#if defaultCheckoutProduct}
            <p class="mt-1 text-xs text-muted-foreground">
              Default checkout product: {defaultCheckoutProduct.name}
            </p>
          {/if}
        </div>
        <div class="rounded-2xl border border-border/70 bg-background/70 p-4">
          <p class="text-xs uppercase tracking-[0.2em] text-muted-foreground">Portal</p>
          <p class="mt-2 text-sm text-muted-foreground">
            Use the hosted Polar portal to manage invoices, payment methods and cancellations without exposing card details in Mikrom.
          </p>
        </div>
      </CardContent>
    </Card>

    <Card>
    <CardHeader>
      <div class="flex items-start justify-between gap-3">
        <div class="flex flex-col gap-1">
          <CardTitle>Polar products</CardTitle>
          <CardDescription>Products available for checkout in Polar.</CardDescription>
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
    <CardContent>
        <p class="mb-4 text-xs text-muted-foreground">
          Last synced: {formatSyncedAt(lastSyncedAt)}
          {#if !canManageBilling}
            {" "}(admin only)
          {/if}
        </p>
        {#if productsLoading}
          <div class="grid gap-3">
            <Skeleton class="h-20 w-full rounded-2xl" />
            <Skeleton class="h-20 w-full rounded-2xl" />
          </div>
        {:else if products.length > 0}
          <div class="flex flex-col gap-4">
            <Field label="Checkout product" forId="checkout_product" description="This is the product opened by the Change plan action.">
              <Select
                bind:value={selectedCheckoutProductId}
                disabled={actionLoading || selectionLoading || !canManageBilling}
                onValueChange={(value: string | undefined) => void handleCheckoutProductChange(value || null)}
              >
                <SelectTrigger id="checkout_product">
                  <SelectValue placeholder="Select a checkout product" />
                </SelectTrigger>
                <SelectContent>
                  {#each products as product}
                    <SelectItem value={product.id}>
                      {product.name}
                      {#if product.is_default_checkout_product}
                        {" "}Default
                      {/if}
                      {#if product.is_archived}
                        {" "}Archived
                      {/if}
                    </SelectItem>
                  {/each}
                </SelectContent>
              </Select>
            </Field>
            {#if !canManageBilling}
              <p class="text-xs text-muted-foreground">
                Only tenant admins can change the checkout product or refresh the catalog.
              </p>
            {/if}

            {#each products as product}
              <div class="rounded-2xl border border-border/70 bg-background/70 p-4">
                <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                  <div class="min-w-0">
                    <div class="flex flex-wrap items-center gap-2">
                      <p class="text-sm font-semibold">{product.name}</p>
                      {#if product.is_default_checkout_product}
                        <Badge variant="secondary">Default</Badge>
                      {/if}
                      {#if product.is_archived}
                        <Badge variant="outline">Archived</Badge>
                      {/if}
                    </div>
                    <p class="mt-1 truncate text-xs text-muted-foreground">{product.id}</p>
                    {#if product.description}
                      <p class="mt-2 text-sm text-muted-foreground">{product.description}</p>
                    {/if}
                  </div>
                  <div class="flex shrink-0 flex-col items-start gap-1 text-sm sm:items-end">
                    <span class="font-medium">
                      {product.price_amount_cents === null || !product.currency
                        ? "Price not configured"
                        : new Intl.NumberFormat("en-US", {
                            style: "currency",
                            currency: product.currency,
                            maximumFractionDigits: 2,
                          }).format(product.price_amount_cents / 100)}
                    </span>
                    <span class="text-xs text-muted-foreground">
                      {product.recurring_interval ? `Billed ${product.recurring_interval}` : "One-time or unspecified billing"}
                    </span>
                  </div>
                </div>
              </div>
            {/each}
          </div>
        {:else}
          <EmptyState class="min-h-[12rem]">
            <p class="text-sm font-medium">No Polar products found</p>
            <p class="text-sm text-muted-foreground">
              Configure products in Polar and make sure the API token can read the catalog.
            </p>
          </EmptyState>
        {/if}
      </CardContent>
    </Card>
  </div>
</div>
