import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

const mocks = vi.hoisted(() => ({
  getToken: vi.fn(),
  getBillingSummary: vi.fn(),
}));

vi.mock("$lib/auth", () => ({
  getToken: mocks.getToken,
}));

vi.mock("$lib/api", () => ({
  getBillingSummary: mocks.getBillingSummary,
}));

import { billingError, billingLoading, billingStore, refreshBilling } from "$lib/stores/billing";

const billingSummary = {
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

beforeEach(() => {
  billingStore.set(null);
  billingLoading.set(false);
  billingError.set("");
  mocks.getToken.mockReset();
  mocks.getBillingSummary.mockReset();
});

describe("billing store", () => {
  it("loads billing data for the current token", async () => {
    mocks.getToken.mockReturnValue("token");
    mocks.getBillingSummary.mockResolvedValue({ data: billingSummary });

    await refreshBilling();

    expect(mocks.getBillingSummary).toHaveBeenCalledWith("token");
    expect(get(billingStore)).toEqual(billingSummary);
    expect(get(billingError)).toBe("");
    expect(get(billingLoading)).toBe(false);
  });

  it("clears billing state when there is no token", async () => {
    billingStore.set(billingSummary);
    billingError.set("stale error");
    billingLoading.set(true);
    mocks.getToken.mockReturnValue(null);

    await refreshBilling();

    expect(get(billingStore)).toBeNull();
    expect(get(billingError)).toBe("");
    expect(get(billingLoading)).toBe(false);
    expect(mocks.getBillingSummary).not.toHaveBeenCalled();
  });
});
