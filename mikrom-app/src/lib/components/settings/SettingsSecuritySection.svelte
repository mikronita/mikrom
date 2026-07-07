<script lang="ts">
  import { getToken } from "$lib/auth";
  import { changePassword, setupTotp, verifyTotp, disableTotp, deleteAccount as deleteAccountApi, type UserProfile } from "$lib/api";
  import { toast } from "$lib/toast";
  import { logout } from "$lib/auth";
  import { goto } from "$app/navigation";
  import {
    AlertDialog,
    Badge,
    Button,
    Card,
    CardContent,
    CardDescription,
    CardFooter,
    CardHeader,
    CardTitle,
    Field,
    Input,
  } from "$lib/components";
  import Check from "@lucide/svelte/icons/check";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import ShieldX from "@lucide/svelte/icons/shield-x";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import X from "@lucide/svelte/icons/x";

  let {
    profile = null,
  }: {
    profile: UserProfile | null;
  } = $props();

  let currentPassword = $state("");
  let newPassword = $state("");
  let changingPassword = $state(false);

  let settingUp2fa = $state(false);
  let totpSecret = $state("");
  let totpUrl = $state("");
  let totpCode = $state("");
  let verifying2fa = $state(false);
  let disabling2fa = $state(false);

  let showDeleteDialog = $state(false);
  let deletingAccount = $state(false);

  let totpEnabled = $derived(profile?.totp_enabled ?? false);

  async function handleChangePassword() {
    const token = getToken();
    if (!token) return;

    if (!currentPassword || !newPassword) {
      toast.error("Both current and new password are required");
      return;
    }

    if (newPassword.length < 8) {
      toast.error("New password must be at least 8 characters");
      return;
    }

    changingPassword = true;
    const result = await changePassword(token, {
      current_password: currentPassword,
      new_password: newPassword,
    });
    changingPassword = false;

    if (result.error) {
      toast.error(result.error);
      return;
    }

    toast.success("Password updated successfully");
    currentPassword = "";
    newPassword = "";
  }

  async function handleSetupTotp() {
    const token = getToken();
    if (!token) return;

    settingUp2fa = true;
    const result = await setupTotp(token);
    settingUp2fa = false;

    if (result.error) {
      toast.error(result.error);
      return;
    }

    if (result.data) {
      totpSecret = result.data.secret;
      totpUrl = result.data.otpauth_url;
    }
  }

  async function handleVerifyTotp() {
    const token = getToken();
    if (!token) return;

    if (!totpCode || totpCode.length !== 6) {
      toast.error("Please enter a valid 6-digit code");
      return;
    }

    verifying2fa = true;
    const result = await verifyTotp(token, { code: totpCode });
    verifying2fa = false;

    if (result.error) {
      toast.error(result.error);
      return;
    }

    toast.success("Two-factor authentication enabled");
    totpSecret = "";
    totpUrl = "";
    totpCode = "";
  }

  function handleCancelTotpSetup() {
    totpSecret = "";
    totpUrl = "";
    totpCode = "";
  }

  async function handleDisableTotp() {
    const token = getToken();
    if (!token) return;

    disabling2fa = true;
    const result = await disableTotp(token);
    disabling2fa = false;

    if (result.error) {
      toast.error(result.error);
      return;
    }

    toast.success("Two-factor authentication disabled");
  }

  async function handleDeleteAccount() {
    const token = getToken();
    if (!token) return;

    deletingAccount = true;
    const result = await deleteAccountApi(token);
    deletingAccount = false;
    showDeleteDialog = false;

    if (result.error) {
      toast.error(result.error);
      return;
    }

    toast.success("Account deleted");
    logout();
    void goto("/auth/login");
  }
</script>

<div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(20rem,0.8fr)]">
  <Card size="sm">
    <CardHeader>
      <CardTitle>Change password</CardTitle>
      <CardDescription>Use a strong password that you do not use anywhere else.</CardDescription>
    </CardHeader>
    <CardContent>
      <div class="flex max-w-md flex-col gap-4">
        <Field label="Current password">
          <Input type="password" bind:value={currentPassword} />
        </Field>
        <Field label="New password">
          <Input type="password" bind:value={newPassword} />
        </Field>
      </div>
    </CardContent>
    <CardFooter>
      <Button onclick={handleChangePassword} disabled={changingPassword}>
        {changingPassword ? "Updating..." : "Update password"}
      </Button>
    </CardFooter>
  </Card>

  <div class="flex flex-col gap-4">
    <Card size="sm">
      <CardHeader class="flex flex-row items-start justify-between gap-4">
        <div class="flex flex-col gap-1.5">
          <CardTitle>Two-factor authentication</CardTitle>
          <CardDescription>Add an extra layer of security to your account.</CardDescription>
        </div>
        {#if totpEnabled}
          <Badge variant="outline" class="gap-1 text-green-600">
            <Check class="size-4" />
            Enabled
          </Badge>
        {:else}
          <Badge variant="outline" class="gap-1">
            <ShieldX class="size-4" />
            Not enabled
          </Badge>
        {/if}
      </CardHeader>
      <CardFooter>
        {#if totpUrl}
          <div class="flex w-full flex-col gap-4">
            <p class="text-sm text-muted-foreground">
              Scan this QR code with your authenticator app (e.g. Google Authenticator, Authy):
            </p>
            <div class="flex justify-center">
              <img src={totpUrl} alt="TOTP QR Code" class="size-40 rounded border" />
            </div>
            <div class="text-center">
              <p class="text-xs text-muted-foreground">Or enter this secret manually:</p>
              <code class="select-all rounded bg-muted px-2 py-1 text-xs font-mono">{totpSecret}</code>
            </div>
            <Field label="Authenticator code">
              <Input
                type="text"
                inputmode="numeric"
                maxlength={6}
                placeholder="000000"
                bind:value={totpCode}
              />
            </Field>
            <div class="flex gap-2">
              <Button onclick={handleVerifyTotp} disabled={verifying2fa} class="flex-1">
                {verifying2fa ? "Verifying..." : "Verify & enable"}
              </Button>
              <Button variant="outline" onclick={handleCancelTotpSetup} disabled={verifying2fa}>
                <X class="size-4" />
                Cancel
              </Button>
            </div>
          </div>
        {:else if totpEnabled}
          <Button size="sm" variant="outline" onclick={handleDisableTotp} disabled={disabling2fa}>
            <ShieldX class="size-4" />
            {disabling2fa ? "Disabling..." : "Disable 2FA"}
          </Button>
        {:else}
          <Button size="sm" onclick={handleSetupTotp} disabled={settingUp2fa}>
            <ShieldCheck class="size-4" />
            {settingUp2fa ? "Setting up..." : "Configure 2FA"}
          </Button>
        {/if}
      </CardFooter>
    </Card>

    <Card size="sm">
      <CardHeader>
        <CardTitle class="text-destructive">Danger zone</CardTitle>
        <CardDescription>Once you delete your account, there is no going back. Please be certain.</CardDescription>
      </CardHeader>
      <CardFooter>
        <Button
          variant="destructive"
          size="sm"
          onclick={() => (showDeleteDialog = true)}
        >
          <Trash2 class="size-4" />
          Delete account
        </Button>
      </CardFooter>
    </Card>
  </div>
</div>

<AlertDialog
  bind:open={showDeleteDialog}
  title="Delete account"
  description="This will permanently delete your account and all associated data. You will not be able to recover it."
  confirmLabel="Delete account"
  confirmVariant="destructive"
  actionText={deletingAccount ? "Deleting..." : "Delete account"}
  loading={deletingAccount}
  onconfirm={handleDeleteAccount}
/>
