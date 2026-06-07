export type BillingStatus = "none" | "active" | "trialing" | "past_due" | "canceled" | "deleted";

export type BillingStatusConfig = {
  tone: string;
  label: string;
  message: string;
  primaryLabel: string;
  secondaryLabel: string;
  primaryHandler: "changePlan" | "manageBilling";
  secondaryHandler: "changePlan" | "manageBilling";
};

export const BILLING_STATUS_CONFIG: Record<BillingStatus, BillingStatusConfig> = {
  none: {
    tone: "border-transparent bg-muted text-muted-foreground",
    label: "Not subscribed",
    message: "",
    primaryLabel: "Start subscription",
    secondaryLabel: "Manage billing",
    primaryHandler: "changePlan",
    secondaryHandler: "manageBilling",
  },
  active: {
    tone: "border-transparent bg-status-info/10 text-status-info",
    label: "Active",
    message: "",
    primaryLabel: "Change plan",
    secondaryLabel: "Manage billing",
    primaryHandler: "changePlan",
    secondaryHandler: "manageBilling",
  },
  trialing: {
    tone: "border-transparent bg-status-info/10 text-status-info",
    label: "Trial",
    message: "You are in a trial period. Choose a plan now so the transition to paid billing is ready before the trial ends.",
    primaryLabel: "Choose plan",
    secondaryLabel: "Manage billing",
    primaryHandler: "changePlan",
    secondaryHandler: "manageBilling",
  },
  past_due: {
    tone: "border-transparent bg-amber-500/10 text-amber-500",
    label: "Past due",
    message: "Payment failed. Open the Polar portal to update the payment method before the subscription is paused.",
    primaryLabel: "Update payment method",
    secondaryLabel: "Change plan",
    primaryHandler: "manageBilling",
    secondaryHandler: "changePlan",
  },
  canceled: {
    tone: "border-transparent bg-destructive/10 text-destructive",
    label: "Canceled",
    message: "This subscription is no longer active. Reactivate it to restore billing for this project.",
    primaryLabel: "Reactivate subscription",
    secondaryLabel: "Manage billing",
    primaryHandler: "changePlan",
    secondaryHandler: "manageBilling",
  },
  deleted: {
    tone: "border-transparent bg-destructive/10 text-destructive",
    label: "Deleted",
    message: "This subscription is no longer active. Reactivate it to restore billing for this project.",
    primaryLabel: "Reactivate subscription",
    secondaryLabel: "Manage billing",
    primaryHandler: "changePlan",
    secondaryHandler: "manageBilling",
  },
};

export function getBillingStatusConfig(status: string | null | undefined): BillingStatusConfig {
  return BILLING_STATUS_CONFIG[(status as BillingStatus) || "none"] || BILLING_STATUS_CONFIG.none;
}
