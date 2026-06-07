import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
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
  has_billing_record: true,
} as const;

describe("SettingsBillingSection", () => {
  it("renders billing details and forwards actions", async () => {
    const onChangePlan = vi.fn();
    const onManageBilling = vi.fn();

    render(SettingsBillingSection, {
      props: {
        billing,
        loading: false,
        actionLoading: false,
        error: "",
        onChangePlan,
        onManageBilling,
      },
    });

    expect(screen.getByText("Pro")).toBeTruthy();
    expect(screen.getByText("Active")).toBeTruthy();
    expect(screen.getByText("Billing is managed by Polar. Customer ID: tenant-1")).toBeTruthy();
    expect(screen.getByText("Subscription ID")).toBeTruthy();
    expect(screen.getByText("prod_123")).toBeTruthy();

    const expectedRenewal = new Intl.DateTimeFormat("en-US", {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date(billing.current_period_end));
    expect(screen.getByText(expectedRenewal)).toBeTruthy();

    await fireEvent.click(screen.getByRole("button", { name: "Change plan" }));
    await fireEvent.click(screen.getByRole("button", { name: "Manage billing" }));

    expect(onChangePlan).toHaveBeenCalledTimes(1);
    expect(onManageBilling).toHaveBeenCalledTimes(1);
  });

  it("disables the primary action when no checkout product is configured", () => {
    render(SettingsBillingSection, {
      props: {
        billing: {
          ...billing,
          default_checkout_product_id: null,
          has_billing_record: false,
          status: "none",
          plan_name: null,
          polar_subscription_id: null,
          polar_product_id: null,
          amount_cents: null,
          currency: null,
          current_period_end: null,
        },
        loading: false,
        actionLoading: false,
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
        },
        loading: false,
        actionLoading: false,
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
        },
        loading: false,
        actionLoading: false,
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

  it("shows a conversion CTA for trialing subscriptions", async () => {
    const onChangePlan = vi.fn();
    const onManageBilling = vi.fn();

    render(SettingsBillingSection, {
      props: {
        billing: {
          ...billing,
          status: "trialing",
        },
        loading: false,
        actionLoading: false,
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
