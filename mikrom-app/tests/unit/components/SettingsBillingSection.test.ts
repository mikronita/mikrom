import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import SettingsBillingSection from "$lib/components/settings/SettingsBillingSection.svelte";

const billing = {
  tenant_id: "tenant-1",
  customer_external_id: "tenant-1",
  polar_customer_id: "cus_123",
  polar_subscription_id: "sub_123",
  polar_product_id: "prod_123",
  plan_name: "Pro",
  status: "active",
  amount_cents: 2500,
  currency: "usd",
  current_period_start: "2026-05-01T00:00:00.000Z",
  current_period_end: "2026-06-01T00:00:00.000Z",
  cancel_at_period_end: false,
  default_checkout_product_id: "prod_default",
  selected_checkout_product_id: null,
  has_billing_record: true,
} as const;

const products = [
  {
    id: "prod_default",
    name: "Pro",
    description: "Production tier",
    price_amount_cents: 2500,
    currency: "usd",
    recurring_interval: "month",
    is_archived: false,
    is_default_checkout_product: true,
  },
  {
    id: "prod_extra",
    name: "Team",
    description: "Extra seats",
    price_amount_cents: 5000,
    currency: "usd",
    recurring_interval: "month",
    is_archived: false,
    is_default_checkout_product: false,
  },
] satisfies import("$lib/api").BillingProduct[];

describe("SettingsBillingSection", () => {
  it("renders billing details and forwards actions", async () => {
    const onChangePlan = vi.fn();
    const onManageBilling = vi.fn();

    render(SettingsBillingSection, {
      props: {
        billing,
        products,
        productsLoading: false,
        loading: false,
        actionLoading: false,
        selectionLoading: false,
        error: "",
        onChangePlan,
        onManageBilling,
      },
    });

    expect(screen.getByText("Pro", { selector: "[data-slot='card-title']" })).toBeTruthy();
    expect(screen.getByText("Active")).toBeTruthy();
    expect(screen.getByText("Billing is managed by Polar. Customer ID: tenant-1")).toBeTruthy();
    expect(screen.getByText("Subscription ID")).toBeTruthy();
    expect(screen.getByText("prod_123")).toBeTruthy();

    const expectedRenewal = new Intl.DateTimeFormat("en-US", {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date(billing.current_period_end));
    expect(screen.getByText(expectedRenewal)).toBeTruthy();

    await waitFor(() => {
      expect((screen.getByRole("button", { name: "Change plan" }) as HTMLButtonElement).disabled).toBe(false);
    });

    await fireEvent.click(screen.getByRole("button", { name: "Change plan" }));
    await fireEvent.click(screen.getByRole("button", { name: "Manage billing" }));

    expect(onChangePlan).toHaveBeenCalledWith("prod_default");
    expect(onManageBilling).toHaveBeenCalledTimes(1);
  });

  it("disables the primary action when no checkout product is configured", () => {
    render(SettingsBillingSection, {
      props: {
        billing: {
          ...billing,
          default_checkout_product_id: null,
          selected_checkout_product_id: null,
          has_billing_record: false,
          status: "none",
          plan_name: null,
          polar_subscription_id: null,
          polar_product_id: null,
          amount_cents: null,
          currency: null,
          current_period_end: null,
        },
        products: [],
        productsLoading: false,
        loading: false,
        actionLoading: false,
        selectionLoading: false,
        error: "",
      },
    });

    expect((screen.getByRole("button", { name: "Start subscription" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("Not subscribed")).toBeTruthy();
    expect(screen.getByText("Not configured")).toBeTruthy();
  });

  it("shows a warning for past due subscriptions", async () => {
    const onChangePlan = vi.fn();
    const onManageBilling = vi.fn();

    render(SettingsBillingSection, {
      props: {
        billing: {
          ...billing,
          status: "past_due",
          selected_checkout_product_id: null,
        },
        products,
        productsLoading: false,
        loading: false,
        actionLoading: false,
        selectionLoading: false,
        error: "",
        onChangePlan,
        onManageBilling,
      },
    });

    expect(screen.getByText("Past due")).toBeTruthy();
    expect(screen.getByText(/Payment failed\./)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Update payment method" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Change plan" })).toBeTruthy();

    await fireEvent.click(screen.getByRole("button", { name: "Update payment method" }));

    expect(onManageBilling).toHaveBeenCalledTimes(1);
    expect(onChangePlan).not.toHaveBeenCalled();
  });

  it("keeps the billing portal available when checkout is not configured", async () => {
    const onChangePlan = vi.fn();
    const onManageBilling = vi.fn();

    render(SettingsBillingSection, {
      props: {
        billing: {
          ...billing,
          status: "past_due",
          default_checkout_product_id: null,
          selected_checkout_product_id: null,
        },
        products,
        productsLoading: false,
        loading: false,
        actionLoading: false,
        selectionLoading: false,
        error: "",
        onChangePlan,
        onManageBilling,
      },
    });

    expect(screen.getByRole("button", { name: "Update payment method" })).toBeTruthy();
    expect((screen.getByRole("button", { name: "Update payment method" }) as HTMLButtonElement).disabled).toBe(false);

    await fireEvent.click(screen.getByRole("button", { name: "Update payment method" }));

    expect(onManageBilling).toHaveBeenCalledTimes(1);
    expect(onChangePlan).not.toHaveBeenCalled();
  });

  it("uses the persisted checkout product from the billing summary", async () => {
    const onChangePlan = vi.fn();
    const onManageBilling = vi.fn();

    render(SettingsBillingSection, {
      props: {
        billing: {
          ...billing,
          selected_checkout_product_id: "prod_extra",
        },
        products,
        productsLoading: false,
        loading: false,
        actionLoading: false,
        selectionLoading: false,
        error: "",
        onChangePlan,
        onManageBilling,
      },
    });

    await fireEvent.click(screen.getByRole("button", { name: "Change plan" }));

    expect(onChangePlan).toHaveBeenCalledWith("prod_extra");
    expect(onManageBilling).not.toHaveBeenCalled();
  });

  it("shows a reactivation CTA for canceled subscriptions", async () => {
    const onChangePlan = vi.fn();
    const onManageBilling = vi.fn();

    render(SettingsBillingSection, {
      props: {
        billing: {
          ...billing,
          status: "canceled",
          plan_name: null,
          polar_subscription_id: null,
          polar_product_id: null,
          selected_checkout_product_id: null,
        },
        products,
        productsLoading: false,
        loading: false,
        actionLoading: false,
        selectionLoading: false,
        error: "",
        onChangePlan,
        onManageBilling,
      },
    });

    expect(screen.getByText("Canceled")).toBeTruthy();
    expect(screen.getByText(/Reactivate it/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Reactivate subscription" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Manage billing" })).toBeTruthy();

    await fireEvent.click(screen.getByRole("button", { name: "Reactivate subscription" }));

    expect(onChangePlan).toHaveBeenCalledTimes(1);
    expect(onManageBilling).not.toHaveBeenCalled();
  });

  it("disables catalog editing for non-admin users", () => {
    render(SettingsBillingSection, {
      props: {
        billing,
        products,
        productsLoading: false,
        loading: false,
        actionLoading: false,
        selectionLoading: false,
        canManageBilling: false,
        error: "",
      },
    });

    expect((screen.getByRole("button", { name: "Refresh" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/Only tenant admins can change the checkout product/)).toBeTruthy();
  });

  it("shows a conversion CTA for trialing subscriptions", async () => {
    const onChangePlan = vi.fn();
    const onManageBilling = vi.fn();

    render(SettingsBillingSection, {
      props: {
        billing: {
          ...billing,
          status: "trialing",
          selected_checkout_product_id: null,
        },
        products,
        productsLoading: false,
        loading: false,
        actionLoading: false,
        selectionLoading: false,
        error: "",
        onChangePlan,
        onManageBilling,
      },
    });

    expect(screen.getByText("Trial")).toBeTruthy();
    expect(screen.getByText(/Choose a plan now/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Choose plan" })).toBeTruthy();

    await fireEvent.click(screen.getByRole("button", { name: "Choose plan" }));

    expect(onChangePlan).toHaveBeenCalledTimes(1);
    expect(onManageBilling).not.toHaveBeenCalled();
  });
});
