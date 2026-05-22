<script lang="ts">
  import { onMount } from "svelte";
  import { Bell, CheckCircle2, CloudDownload, CreditCard, Github, KeyRound, Loader2, Mail, Plus, Puzzle, Settings, ShieldCheck, Trash2, User } from "lucide-svelte";
  import {
    Card,
    CardHeader,
    CardTitle,
    CardDescription,
    CardContent,
    CardFooter,
    Badge,
    Avatar,
    AvatarFallback,
    Button,
    CardSkeleton,
    Field,
    Input,
    Separator,
    Skeleton,
    Switch,
  } from "$lib/components";
  import DashboardLayout from "$lib/components/DashboardLayout.svelte";
  import { getToken } from "$lib/auth";
  import { getGithubInstallUrl, getUserProfile, listGithubAccounts, updateUserProfile, type GithubAccount, type UserProfile } from "$lib/api";
  import { toast } from "$lib/toast";

  const settingsTabs = [
    { value: "profile", label: "Profile", icon: User },
    { value: "security", label: "Security", icon: KeyRound },
    { value: "api", label: "API access", icon: CloudDownload },
    { value: "billing", label: "Billing", icon: CreditCard },
    { value: "integrations", label: "Integrations", icon: Puzzle },
    { value: "notifications", label: "Notifications", icon: Bell },
  ] as const;

  type TabValue = (typeof settingsTabs)[number]["value"];

  let activeTab: TabValue = "profile";
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

    const [profileResult, githubResult] = await Promise.all([getUserProfile(token), listGithubAccounts(token)]);
    if (profileResult.data) {
      profile = profileResult.data;
      firstNameDraft = profile.first_name || "";
      lastNameDraft = profile.last_name || "";
    }
    if (githubResult.data) githubAccounts = githubResult.data;
    loading = false;
    loadingGithub = false;
  });

  const initials = () =>
    profile ? `${profile.first_name?.[0] || ""}${profile.last_name?.[0] || ""}`.toUpperCase() || profile.email?.[0]?.toUpperCase() || "U" : "U";

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
        <p class="max-w-2xl text-sm text-muted-foreground">Manage your personal information, security preferences and billing.</p>
      </div>
    </div>

    <div class="grid h-auto w-full grid-cols-2 gap-1 overflow-hidden rounded-lg border border-border bg-muted p-1 sm:grid-cols-3 xl:grid-cols-6">
      {#each settingsTabs as tab}
        <button
          class={`flex min-h-10 items-center justify-start gap-2 rounded-md px-3 text-sm transition-colors sm:justify-center ${
            activeTab === tab.value ? "bg-background shadow-sm text-foreground" : "text-muted-foreground hover:text-foreground"
          }`}
          on:click={() => (activeTab = tab.value)}
        >
          <svelte:component this={tab.icon} class="size-4 shrink-0" />
          <span class="truncate">{tab.label}</span>
        </button>
      {/each}
    </div>

    {#if activeTab === "profile"}
      <Card class="overflow-hidden">
        <CardHeader>
          <CardTitle>Profile</CardTitle>
          <CardDescription>Update the public name and contact details for your account.</CardDescription>
        </CardHeader>
        <CardContent>
          {#if loading}
            <div class="flex flex-col gap-8">
              <CardSkeleton
                showBadge={false}
                iconClassName="size-20 rounded-full"
                titleClassName="w-36"
                descriptionClassName="w-56"
                footerLineClassName=""
                footerPills={["w-24", "w-20"]}
              />

              <Separator />

              <div class="grid gap-6 md:grid-cols-2">
                <Skeleton class="h-20 w-full" />
                <Skeleton class="h-20 w-full" />
                <div class="md:col-span-2">
                  <Skeleton class="h-20 w-full" />
                </div>
              </div>
            </div>
          {:else}
            <div class="flex flex-col gap-8">
              <div class="flex flex-col items-start gap-5 sm:flex-row sm:items-center">
                <Avatar class="size-20">
                  <AvatarFallback class="text-xl font-semibold">
                    {initials()}
                  </AvatarFallback>
                </Avatar>
                <div class="flex flex-1 flex-col gap-3">
                  <div class="flex flex-col gap-1">
                    <h3 class="text-base font-semibold">Profile picture</h3>
                    <p class="text-sm text-muted-foreground">JPG, GIF or PNG. Max size of 800K.</p>
                  </div>
                  <div class="flex flex-wrap gap-2">
                    <Button size="sm">Upload new</Button>
                    <Button variant="outline" size="sm">Remove</Button>
                  </div>
                </div>
              </div>

              <Separator />

              <div class="grid gap-6 md:grid-cols-2">
                <Field label="First name">
                  <Input bind:value={firstNameDraft} placeholder="John" />
                </Field>
                <Field label="Last name">
                  <Input bind:value={lastNameDraft} placeholder="Doe" />
                </Field>
                <div class="md:col-span-2">
                  <Field label="Email address" description="Email cannot be changed yet.">
                    <div class="flex h-9 items-center gap-2 rounded-md border border-input bg-background px-3 shadow-none transition-colors focus-within:ring-2 focus-within:ring-ring">
                      <Mail class="size-4 shrink-0 text-muted-foreground" />
                      <Input value={profile?.email || ""} disabled class="border-0 bg-transparent px-0 focus-visible:ring-0" />
                    </div>
                  </Field>
                </div>
              </div>
            </div>
          {/if}
        </CardContent>
        <CardFooter class="justify-end">
          <Button onclick={saveProfile} disabled={loading || saving}>
            {#if saving}
              <Loader2 class="size-4 animate-spin" />
            {/if}
            Save changes
          </Button>
        </CardFooter>
      </Card>
    {:else if activeTab === "security"}
      <div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(20rem,0.8fr)]">
        <Card class="overflow-hidden">
          <CardHeader>
            <CardTitle>Change password</CardTitle>
            <CardDescription>Use a strong password that you do not use anywhere else.</CardDescription>
          </CardHeader>
          <CardContent>
            <div class="max-w-md space-y-4">
              <Field label="Current password">
                <Input type="password" />
              </Field>
              <Field label="New password">
                <Input type="password" />
              </Field>
            </div>
          </CardContent>
          <CardFooter>
            <Button>Update password</Button>
          </CardFooter>
        </Card>

        <div class="space-y-4">
          <Card class="overflow-hidden">
            <CardHeader class="flex flex-row items-start justify-between gap-4">
              <div class="flex flex-col gap-1.5">
                <CardTitle>Two-factor authentication</CardTitle>
                <CardDescription>Add an extra layer of security to your account.</CardDescription>
              </div>
              <Badge variant="outline">
                <ShieldCheck class="size-4" />
                Not enabled
              </Badge>
            </CardHeader>
            <CardFooter>
              <Button size="sm">Configure 2FA</Button>
            </CardFooter>
          </Card>

          <Card class="overflow-hidden">
            <CardHeader>
              <CardTitle class="text-destructive">Danger zone</CardTitle>
              <CardDescription>Once you delete your account, there is no going back. Please be certain.</CardDescription>
            </CardHeader>
            <CardFooter>
              <Button variant="destructive" size="sm">
                <Trash2 class="size-4" />
                Delete account
              </Button>
            </CardFooter>
          </Card>
        </div>
      </div>
    {:else if activeTab === "api"}
      <Card class="overflow-hidden">
        <CardHeader class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
          <div class="flex flex-col gap-1.5">
            <CardTitle>Personal access tokens</CardTitle>
            <CardDescription>Use tokens to authenticate with the Mikrom CLI and API.</CardDescription>
          </div>
          <Button size="sm">
            <Plus class="size-4" />
            Create token
          </Button>
        </CardHeader>
        <CardContent>
          <div class="flex flex-col gap-4">
            <div class="flex flex-col gap-4 rounded-lg border bg-muted/30 p-4 sm:flex-row sm:items-center sm:justify-between">
              <div class="flex items-center gap-4">
                <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
                  <CheckCircle2 class="size-4" />
                </div>
                <div class="min-w-0">
                  <p class="truncate font-mono text-sm font-semibold">mikrom_pk_live_****************</p>
                  <p class="text-xs text-muted-foreground">Last used 2 hours ago - Created April 12, 2026</p>
                </div>
              </div>
              <Button variant="destructive" size="sm">Revoke</Button>
            </div>
          </div>
        </CardContent>
      </Card>
    {:else if activeTab === "billing"}
      <div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(20rem,0.85fr)]">
        <Card class="overflow-hidden">
          <CardHeader class="border-b bg-muted/30">
            <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
              <div class="flex flex-col gap-2">
                <CardDescription>Current plan</CardDescription>
                <CardTitle class="text-2xl">Pro developer</CardTitle>
                <p class="text-sm text-muted-foreground">$29 / month</p>
              </div>
              <Badge variant="outline" class="border-transparent bg-[color-mix(in_srgb,var(--status-info)_12%,transparent)] text-[var(--status-info)]">Active</Badge>
            </div>
          </CardHeader>
          <CardContent class="pt-6">
            <div class="grid gap-3 sm:grid-cols-3">
              <div class="rounded-lg border bg-background p-4">
                <p class="text-2xl font-semibold">4</p>
                <p class="text-sm text-muted-foreground">Active apps</p>
              </div>
              <div class="rounded-lg border bg-background p-4">
                <p class="text-2xl font-semibold">120 GB</p>
                <p class="text-sm text-muted-foreground">Bandwidth</p>
              </div>
              <div class="rounded-lg border bg-background p-4">
                <p class="text-2xl font-semibold">24/7</p>
                <p class="text-sm text-muted-foreground">Support</p>
              </div>
            </div>
          </CardContent>
          <CardFooter class="flex flex-wrap gap-2">
            <Button size="sm">Change plan</Button>
            <Button variant="outline" size="sm">Cancel subscription</Button>
          </CardFooter>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Payment method</CardTitle>
            <CardDescription>Manage the card used for renewals and invoices.</CardDescription>
          </CardHeader>
          <CardContent>
            <div class="flex items-center gap-4 rounded-lg border bg-muted/30 p-4">
              <div class="flex h-8 w-12 shrink-0 items-center justify-center rounded-md border bg-background text-xs font-semibold italic text-muted-foreground">
                VISA
              </div>
              <div class="min-w-0 flex-1">
                <p class="truncate text-sm font-semibold">Visa ending in 4242</p>
                <p class="text-xs text-muted-foreground">Expires 12/28</p>
              </div>
              <Button variant="outline" size="sm">Edit</Button>
            </div>
          </CardContent>
        </Card>
      </div>
    {:else if activeTab === "integrations"}
      <Card class="overflow-hidden">
        <CardHeader>
          <CardTitle>Integrations</CardTitle>
          <CardDescription>Connect source control providers and external services.</CardDescription>
        </CardHeader>
        <CardContent>
          <div class="space-y-1.5">
            <p class="text-sm font-medium">Source control</p>
            <p class="text-sm text-muted-foreground">Connect your GitHub account to deploy private repositories.</p>

            <div class="mt-4 space-y-4">
              {#if loadingGithub}
                <div class="flex flex-col gap-4">
                  <CardSkeleton
                    compact
                    showBadge={false}
                    iconClassName="size-10 rounded-lg"
                    titleClassName="w-32"
                    descriptionClassName="w-44"
                    footerLineClassName=""
                  />
                  <CardSkeleton
                    compact
                    showBadge={false}
                    iconClassName="size-10 rounded-lg"
                    titleClassName="w-32"
                    descriptionClassName="w-44"
                    footerLineClassName=""
                  />
                </div>
              {:else if githubAccounts.length > 0}
                {#each githubAccounts as account}
                  <div class="flex flex-col gap-4 rounded-lg border bg-muted/30 p-4 sm:flex-row sm:items-center sm:justify-between">
                    <div class="flex items-center gap-4">
                      <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
                        <Github class="size-4" />
                      </div>
                      <div class="min-w-0">
                        <p class="truncate text-sm font-semibold">@{account.github_username}</p>
                        <p class="text-xs text-muted-foreground">Connected on {new Date(account.created_at).toLocaleDateString()}</p>
                      </div>
                    </div>
                    <Button variant="outline" size="sm" href={`https://github.com/settings/installations/${account.installation_id}`} target="_blank" rel="noreferrer">
                      Configure
                    </Button>
                  </div>
                {/each}
                <div>
                  <Button variant="outline" size="sm" onclick={connectGithub}>
                    <Plus class="size-4" />
                    Connect another account
                  </Button>
                </div>
              {:else}
                <div class="flex flex-col gap-4 rounded-lg border bg-muted/30 p-4 sm:flex-row sm:items-center sm:justify-between">
                  <div class="flex items-center gap-4">
                    <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
                      <Github class="size-4" />
                    </div>
                    <div class="min-w-0">
                      <p class="text-sm font-semibold">GitHub app integration</p>
                      <p class="text-xs text-muted-foreground">Deploy from any repository you have access to.</p>
                    </div>
                  </div>
                  <Button size="sm" onclick={connectGithub}>Connect GitHub</Button>
                </div>
              {/if}
            </div>
          </div>
        </CardContent>
      </Card>
    {:else if activeTab === "notifications"}
      <Card class="overflow-hidden">
        <CardHeader>
          <CardTitle>Notifications</CardTitle>
          <CardDescription>Choose what updates you want to receive via email.</CardDescription>
        </CardHeader>
        <CardContent>
          <div class="space-y-4">
            <div class="flex items-start justify-between gap-4 rounded-lg border bg-muted/30 p-4">
              <div class="space-y-1">
                <div class="text-base font-medium">Deployment status</div>
                <p class="text-sm text-muted-foreground">Receive an email when your deployments finish or fail.</p>
              </div>
              <Switch bind:checked={emailNotifications} aria-label="Toggle deployment status notifications" />
            </div>
            <div class="flex items-start justify-between gap-4 rounded-lg border bg-muted/30 p-4">
              <div class="space-y-1">
                <div class="text-base font-medium">Marketing emails</div>
                <p class="text-sm text-muted-foreground">New features, tips and weekly summaries.</p>
              </div>
              <Switch bind:checked={marketingEmails} aria-label="Toggle marketing emails" />
            </div>
          </div>
        </CardContent>
      </Card>
    {/if}
  </div>
</DashboardLayout>
