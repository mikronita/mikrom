<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/stores";
    import Settings from "lucide-svelte/icons/settings";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import SettingsApiSection from "$lib/components/settings/SettingsApiSection.svelte";
  import SettingsBillingSection from "$lib/components/settings/SettingsBillingSection.svelte";
  import SettingsIntegrationsSection from "$lib/components/settings/SettingsIntegrationsSection.svelte";
  import SettingsNotificationsSection from "$lib/components/settings/SettingsNotificationsSection.svelte";
  import SettingsProfileSection from "$lib/components/settings/SettingsProfileSection.svelte";
  import SettingsSecuritySection from "$lib/components/settings/SettingsSecuritySection.svelte";
  import { getToken } from "$lib/auth";
  import {
    createBillingCheckout,
    createBillingPortal,
    getGithubInstallUrl,
    getUserProfile,
    listGithubAccounts,
    listBillingProducts,
    refreshBillingProducts,
    updateBillingCheckoutProduct,
    updateUserProfile,
    uploadUserAvatar,
    resolveAvatarUrl,
    type GithubAccount,
    type BillingProduct,
  } from "$lib/api";
  import { Avatar, AvatarFallback, AvatarImage } from "$lib/components";
  import { getBillingStatusConfig } from "$lib/domain/billing";
  import { toast } from "$lib/toast";
  import { profile, refreshProfile } from "$lib/stores/profile";
  import {
    billing,
    billingError,
    billingLoading,
    billingStore,
    refreshBilling,
    useBillingBootstrap,
  } from "$lib/stores/billing";
  import { settingsTabs, type SettingsTab } from "$lib/domain/settings";
  import { cn } from "$lib/utils";

  let githubAccounts = $state<GithubAccount[]>([]);
  let loading = $state(true);
  let loadingGithub = $state(true);
  let loadingBillingProducts = $state(true);
  let firstNameDraft = $state("");
  let lastNameDraft = $state("");
  let emailNotifications = $state(true);
  let marketingEmails = $state(false);
  let saving = $state(false);
  let billingActionLoading = $state(false);
  let checkoutProductSaving = $state(false);
  let billingProductsRefreshing = $state(false);
  let billingProducts = $state<BillingProduct[]>([]);
  let billingProductsLastSyncedAt = $state<string | null>(null);
  let avatarUploading = $state(false);
  let canManageBilling = $derived((($profile?.role || "").toLowerCase()) === "admin");
  let billingStatus = $derived(getBillingStatusConfig($billing?.status));
  let resolvedAvatarUrl = $derived(resolveAvatarUrl($profile?.avatar_url));

  const settingsTabValues = new Set<SettingsTab>(settingsTabs.map((tab) => tab.value));

  function parseSettingsTab(value: string | null): SettingsTab {
    if (value && settingsTabValues.has(value as SettingsTab)) {
      return value as SettingsTab;
    }

    return "profile";
  }

  let activeTab = $derived(parseSettingsTab($page.url.searchParams.get("tab")));
  let billingTabNotice = $derived(
    activeTab === "billing"
      ? $billingError || ($billingLoading ? "Loading billing status..." : "")
      : "",
  );

  async function setActiveTab(tab: SettingsTab) {
    const nextUrl = new URL($page.url);
    nextUrl.searchParams.set("tab", tab);
    nextUrl.searchParams.delete("checkout");

    await goto(nextUrl.toString(), {
      replaceState: true,
      noScroll: true,
      keepFocus: true,
    });
  }

  function ensureValidTab() {
    const tab = $page.url.searchParams.get("tab");
    if (!tab || settingsTabValues.has(tab as SettingsTab)) return;

    const nextUrl = new URL($page.url);
    nextUrl.searchParams.set("tab", "profile");

    void goto(nextUrl.toString(), {
      replaceState: true,
      noScroll: true,
      keepFocus: true,
    });
  }

  onMount(() => {
    ensureValidTab();

    if ($page.url.searchParams.get("checkout") === "success") {
      toast.success("Billing updated successfully");

      const nextUrl = new URL($page.url);
      nextUrl.searchParams.delete("checkout");
      void goto(nextUrl.toString(), {
        replaceState: true,
        noScroll: true,
        keepFocus: true,
      });
    }

    void (async () => {
      const token = getToken();
      if (!token) {
        loading = false;
        loadingGithub = false;
        loadingBillingProducts = false;
        return;
      }

      const [profileResult, githubResult, billingProductsResult] = await Promise.all([
        getUserProfile(token),
        listGithubAccounts(token),
        listBillingProducts(token),
      ]);

      if (profileResult.data) {
        firstNameDraft = profileResult.data.first_name || "";
        lastNameDraft = profileResult.data.last_name || "";
      }

      if (githubResult.data) githubAccounts = githubResult.data;
      if (billingProductsResult.error) {
        toast.error(billingProductsResult.error);
      }
      if (billingProductsResult.data) billingProducts = billingProductsResult.data.products;
      if (billingProductsResult.data) billingProductsLastSyncedAt = billingProductsResult.data.last_synced_at;

      loading = false;
      loadingGithub = false;
      loadingBillingProducts = false;
    })();
  });

  useBillingBootstrap();

  async function saveProfile() {
    const token = getToken();
    if (!token) return;

    saving = true;
    const result = await updateUserProfile(token, {
      first_name: firstNameDraft || null,
      last_name: lastNameDraft || null,
    });
    saving = false;

    if (result.error) {
      toast.error(result.error);
      return;
    }

    if (result.data) {
      firstNameDraft = result.data.first_name || "";
      lastNameDraft = result.data.last_name || "";
      void refreshProfile();
    }
    toast.success("Profile updated successfully");
  }

  async function handleAvatarSelected(event: Event) {
    const target = event.currentTarget as HTMLInputElement;
    const file = target.files?.[0];
    if (!file) return;

    const token = getToken();
    if (!token) return;

    avatarUploading = true;
    const result = await uploadUserAvatar(token, file);
    avatarUploading = false;
    target.value = "";

    if (result.error) {
      toast.error(result.error);
      return;
    }

    if (result.data) {
      void refreshProfile();
    }
    toast.success("Avatar updated successfully");
  }

  async function connectGithub() {
    const token = getToken();
    if (!token) {
      toast.error("You must be logged in to connect GitHub");
      return;
    }

    const result = await getGithubInstallUrl(token);
    if (result.data?.url) {
      window.location.href = result.data.url;
      return;
    }

    toast.error(result.error || "Failed to start GitHub installation");
  }

  async function openBillingCheckout(productId?: string | null) {
    const token = getToken();
    if (!token) {
      toast.error("You must be logged in to manage billing");
      return;
    }

    const checkoutProductId =
      productId || $billing?.selected_checkout_product_id || $billing?.default_checkout_product_id;
    if (!checkoutProductId) {
      toast.error("No Polar product is configured for checkout");
      return;
    }

    billingActionLoading = true;
    try {
      const result = await createBillingCheckout(token, {
        product_id: checkoutProductId,
      });

      if (result.data?.url) {
        window.location.href = result.data.url;
        return;
      }

      toast.error(result.error || "Failed to create checkout session");
    } finally {
      billingActionLoading = false;
    }
  }

  async function updateCheckoutProductSelection(productId?: string | null) {
    const token = getToken();
    if (!token) {
      toast.error("You must be logged in to manage billing");
      return;
    }
    if (!canManageBilling) {
      toast.error("Only tenant admins can change the checkout product");
      return;
    }

    checkoutProductSaving = true;
    try {
      const result = await updateBillingCheckoutProduct(token, {
        product_id: productId || null,
      });

      if (result.error) {
        toast.error(result.error);
        await refreshBilling();
        return;
      }

      if (result.data) {
        billingStore.set(result.data);
        billingError.set("");
      }
    } finally {
      checkoutProductSaving = false;
    }
  }

  async function refreshBillingProductsList() {
    const token = getToken();
    if (!token) {
      toast.error("You must be logged in to refresh billing products");
      return;
    }
    if (!canManageBilling) {
      toast.error("Only tenant admins can refresh the billing catalog");
      return;
    }

    billingProductsRefreshing = true;
    try {
      const result = await refreshBillingProducts(token);
      if (result.error) {
        toast.error(result.error);
        return;
      }

      if (result.data) {
        billingProducts = result.data.products;
        billingProductsLastSyncedAt = result.data.last_synced_at;
        toast.success("Billing products refreshed");
      }
    } finally {
      billingProductsRefreshing = false;
    }
  }

  async function openBillingPortal() {
    const token = getToken();
    if (!token) {
      toast.error("You must be logged in to manage billing");
      return;
    }

    billingActionLoading = true;
    try {
      const result = await createBillingPortal(token);
      if (result.data?.url) {
        window.location.href = result.data.url;
        return;
      }

      toast.error(result.error || "Failed to open billing portal");
    } finally {
      billingActionLoading = false;
    }
  }
</script>

<svelte:head>
  <title>Mikrom - Settings</title>
</svelte:head>

<DashboardLayout>
  <div class="flex flex-col gap-6">
    <div class="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
      <div class="flex flex-col gap-2">
        <div class="flex items-center gap-3">
          <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
            <Settings />
          </div>
          <h1 class="text-3xl font-semibold tracking-tight">Settings</h1>
        </div>
        <p class="max-w-2xl text-sm text-muted-foreground">
          Manage your personal information, security preferences and billing.
        </p>
        {#if activeTab === "billing"}
          <div class="inline-flex w-fit items-center gap-2 rounded-full border border-border bg-background px-3 py-1 text-xs font-medium text-muted-foreground">
            <span class={cn("inline-flex items-center rounded-full px-2 py-0.5", billingStatus.tone)}>
              {billingStatus.label}
            </span>
            <span>Billing status</span>
          </div>
          {#if billingTabNotice}
            <p class="text-xs text-muted-foreground">{billingTabNotice}</p>
          {/if}
        {/if}
      </div>
    </div>

    <nav class="border-b border-border" aria-label="Settings sections">
      <div class="grid h-auto w-full grid-cols-2 gap-0.5 sm:grid-cols-3 xl:grid-cols-6">
        {#each settingsTabs as tab}
          {@const Icon = tab.icon}
          <button
            class={`flex items-center justify-start gap-2 border-b-2 px-4 py-2.5 text-sm font-medium transition-colors sm:justify-center ${
              activeTab === tab.value
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground"
            }`}
            onclick={() => void setActiveTab(tab.value)}
          >
            <Icon class="size-4 shrink-0" />
            <span class="truncate">{tab.label}</span>
          </button>
        {/each}
      </div>
    </nav>

    {#if activeTab === "profile"}
      <SettingsProfileSection
        bind:firstNameDraft
        bind:lastNameDraft
        profile={$profile}
        {loading}
        {saving}
        onSave={saveProfile}
      />
      <div class="mt-6 flex items-center gap-4 rounded-lg border border-border bg-background p-4">
        <Avatar class="size-16">
          <AvatarImage src={resolvedAvatarUrl || undefined} alt="User avatar" />
          <AvatarFallback>
            {($profile?.first_name?.[0] || $profile?.email?.[0] || "U").toUpperCase()}
          </AvatarFallback>
        </Avatar>
        <div class="space-y-2">
          <div class="text-sm font-medium">Profile avatar</div>
          <div class="text-xs text-muted-foreground">PNG, JPG or WebP. Up to a small image file.</div>
          <label class="inline-flex cursor-pointer items-center rounded-md border border-border bg-background px-3 py-2 text-sm font-medium hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50">
            <span>{avatarUploading ? "Uploading..." : "Change avatar"}</span>
            <input type="file" accept="image/png,image/jpeg,image/webp" class="hidden" onchange={handleAvatarSelected} disabled={avatarUploading} />
          </label>
        </div>
      </div>
    {:else if activeTab === "security"}
      <SettingsSecuritySection />
    {:else if activeTab === "api"}
      <SettingsApiSection />
    {:else if activeTab === "billing"}
      <SettingsBillingSection
        billing={$billing}
        products={billingProducts}
        productsLoading={loadingBillingProducts}
        productsRefreshing={billingProductsRefreshing}
        lastSyncedAt={billingProductsLastSyncedAt}
        loading={$billingLoading}
        error={$billingError}
        actionLoading={billingActionLoading}
        selectionLoading={checkoutProductSaving}
        canManageBilling={canManageBilling}
        onChangePlan={openBillingCheckout}
        onCheckoutProductChange={updateCheckoutProductSelection}
        onRefreshProducts={refreshBillingProductsList}
        onManageBilling={openBillingPortal}
      />
    {:else if activeTab === "integrations"}
      <SettingsIntegrationsSection
        {loadingGithub}
        {githubAccounts}
        onConnectGithub={connectGithub}
      />
    {:else if activeTab === "notifications"}
      <SettingsNotificationsSection bind:emailNotifications bind:marketingEmails />
    {/if}
  </div>
</DashboardLayout>
