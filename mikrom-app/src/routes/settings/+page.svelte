<script lang="ts">
  import { onMount } from "svelte";
  import { Settings } from "lucide-svelte";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import SettingsApiSection from "$lib/components/settings/SettingsApiSection.svelte";
  import SettingsBillingSection from "$lib/components/settings/SettingsBillingSection.svelte";
  import SettingsIntegrationsSection from "$lib/components/settings/SettingsIntegrationsSection.svelte";
  import SettingsNotificationsSection from "$lib/components/settings/SettingsNotificationsSection.svelte";
  import SettingsProfileSection from "$lib/components/settings/SettingsProfileSection.svelte";
  import SettingsSecuritySection from "$lib/components/settings/SettingsSecuritySection.svelte";
  import { getToken } from "$lib/auth";
  import {
    getGithubInstallUrl,
    getUserProfile,
    listGithubAccounts,
    updateUserProfile,
    type GithubAccount,
    type UserProfile,
  } from "$lib/api";
  import { toast } from "$lib/toast";
  import { settingsTabs, type SettingsTab } from "$lib/domain/settings";

  let activeTab: SettingsTab = "profile";
  let profile: UserProfile | null = null;
  let githubAccounts: GithubAccount[] = [];
  let loading = true;
  let loadingGithub = true;
  let firstNameDraft = "";
  let lastNameDraft = "";
  let emailNotifications = true;
  let marketingEmails = false;
  let saving = false;

  onMount(async () => {
    const token = getToken();
    if (!token) {
      loading = false;
      loadingGithub = false;
      return;
    }

    const [profileResult, githubResult] = await Promise.all([
      getUserProfile(token),
      listGithubAccounts(token),
    ]);

    if (profileResult.data) {
      profile = profileResult.data;
      firstNameDraft = profile.first_name || "";
      lastNameDraft = profile.last_name || "";
    }

    if (githubResult.data) githubAccounts = githubResult.data;

    loading = false;
    loadingGithub = false;
  });

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

    if (result.data) profile = result.data;
    toast.success("Profile updated successfully");
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
      </div>
    </div>

    <nav class="border-b border-border" aria-label="Settings sections">
      <div class="grid h-auto w-full grid-cols-2 gap-0.5 sm:grid-cols-3 xl:grid-cols-6">
        {#each settingsTabs as tab}
          <button
            class={`flex items-center justify-start gap-2 border-b-2 px-4 py-2.5 text-sm font-medium transition-colors sm:justify-center ${
              activeTab === tab.value
                ? "border-primary text-foreground"
                : "border-transparent text-muted-foreground hover:text-foreground"
            }`}
            on:click={() => (activeTab = tab.value)}
          >
            <svelte:component this={tab.icon} class="size-4 shrink-0" />
            <span class="truncate">{tab.label}</span>
          </button>
        {/each}
      </div>
    </nav>

    {#if activeTab === "profile"}
      <SettingsProfileSection
        bind:firstNameDraft
        bind:lastNameDraft
        {profile}
        {loading}
        {saving}
        onSave={saveProfile}
      />
    {:else if activeTab === "security"}
      <SettingsSecuritySection />
    {:else if activeTab === "api"}
      <SettingsApiSection />
    {:else if activeTab === "billing"}
      <SettingsBillingSection />
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
