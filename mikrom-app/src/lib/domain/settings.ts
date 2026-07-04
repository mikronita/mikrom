import Bell from "lucide-svelte/icons/bell";
import CloudDownload from "lucide-svelte/icons/cloud-download";
import CreditCard from "lucide-svelte/icons/credit-card";
import KeyRound from "lucide-svelte/icons/key-round";
import Puzzle from "lucide-svelte/icons/puzzle";
import User from "lucide-svelte/icons/user";

export const settingsTabs = [
  { value: "profile", label: "Profile", icon: User },
  { value: "security", label: "Security", icon: KeyRound },
  { value: "api", label: "API access", icon: CloudDownload },
  { value: "billing", label: "Billing", icon: CreditCard },
  { value: "integrations", label: "Integrations", icon: Puzzle },
  { value: "notifications", label: "Notifications", icon: Bell },
] as const;

export type SettingsTab = (typeof settingsTabs)[number]["value"];

export function getProfileInitials(
  firstName?: string | null,
  lastName?: string | null,
  email?: string | null,
) {
  const initials = `${firstName?.[0] || ""}${lastName?.[0] || ""}`.toUpperCase();
  return initials || email?.[0]?.toUpperCase() || "U";
}
