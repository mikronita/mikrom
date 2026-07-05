<script lang="ts">
  import Loader2 from "@lucide/svelte/icons/loader-2";
  import Mail from "@lucide/svelte/icons/mail";
  import {
    Avatar,
    AvatarFallback,
    AvatarImage,
    Button,
    Card,
    CardContent,
    CardDescription,
    CardFooter,
    CardHeader,
    CardTitle,
    CardSkeleton,
    Field,
    FieldGroup,
    Input,
    Separator,
    Skeleton,
  } from "$lib/components";
  import { resolveAvatarUrl, type UserProfile } from "$lib/api";
  import { getProfileInitials } from "$lib/domain/settings";

  let {
    profile = null,
    loading = false,
    saving = false,
    avatarUploading = false,
    firstNameDraft = $bindable(""),
    lastNameDraft = $bindable(""),
    onSave,
    onAvatarSelected,
  } = $props<{
    profile?: UserProfile | null;
    loading?: boolean;
    saving?: boolean;
    avatarUploading?: boolean;
    firstNameDraft?: string;
    lastNameDraft?: string;
    onSave: () => Promise<void> | void;
    onAvatarSelected: (event: Event) => Promise<void> | void;
  }>();

  let resolvedAvatarUrl = $derived(resolveAvatarUrl(profile?.avatar_url));
</script>

<Card size="sm">
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
          {#key resolvedAvatarUrl}
            <Avatar class="size-20">
              <AvatarImage src={resolvedAvatarUrl || undefined} alt="User avatar" />
              <AvatarFallback class="text-xl font-semibold">
                {getProfileInitials(profile?.first_name, profile?.last_name, profile?.email)}
              </AvatarFallback>
            </Avatar>
          {/key}
          <div class="flex flex-1 flex-col gap-3">
            <div class="flex flex-col gap-1">
              <h3 class="text-base font-semibold">Profile avatar</h3>
              <p class="text-sm text-muted-foreground">PNG, JPG or WebP. Up to a small image file.</p>
            </div>
            <label class="inline-flex w-fit cursor-pointer items-center rounded-md border border-border bg-background px-3 py-2 text-sm font-medium hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50">
              <span>{avatarUploading ? "Uploading..." : "Change avatar"}</span>
              <input type="file" accept="image/png,image/jpeg,image/webp" class="hidden" onchange={onAvatarSelected} disabled={avatarUploading} />
            </label>
          </div>
        </div>

        <Separator />

        <FieldGroup>
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
        </FieldGroup>
      </div>
    {/if}
  </CardContent>
  <CardFooter class="justify-end">
    <Button onclick={onSave} disabled={loading || saving}>
      {#if saving}
        <Loader2 class="size-4 animate-spin" />
      {/if}
      Save changes
    </Button>
  </CardFooter>
</Card>
