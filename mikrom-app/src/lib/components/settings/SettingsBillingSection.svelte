<script lang="ts">
  import { Badge, Button, Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle, Skeleton } from "$lib/components";
  import type { BillingSummary } from "$lib/api";
  import { getBillingStatusConfig } from "$lib/domain/billing";

  let {
    billing = null,
    loading = false,
    actionLoading = false,
    error = "",
    onChangePlan = () => {},
    onManageBilling = () => {},
  } = $props<{
    billing?: BillingSummary | null;
    loading?: boolean;
    actionLoading?: boolean;
    error?: string;
    onChangePlan?: () => void;
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

  function formatDate(value: string | null) {
    if (!value) return "Not scheduled";
    return new Intl.DateTimeFormat("en-US", {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date(value));
  }

  let currentBilling = $derived(getBillingStatusConfig(billing?.status));
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
        onclick={currentBilling.primaryHandler === "manageBilling" ? onManageBilling : onChangePlan}
        disabled={actionLoading || loading || !billing?.default_checkout_product_id}
      >
        {currentBilling.primaryLabel}
      </Button>
      <Button
        variant="outline"
        size="sm"
        onclick={currentBilling.secondaryHandler === "manageBilling" ? onManageBilling : onChangePlan}
        disabled={actionLoading || loading}
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
      </div>
      <div class="rounded-2xl border border-border/70 bg-background/70 p-4">
        <p class="text-xs uppercase tracking-[0.2em] text-muted-foreground">Portal</p>
        <p class="mt-2 text-sm text-muted-foreground">
          Use the hosted Polar portal to manage invoices, payment methods and cancellations without exposing card details in Mikrom.
        </p>
      </div>
    </CardContent>
  </Card>
</div>
