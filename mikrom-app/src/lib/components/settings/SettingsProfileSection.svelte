<script lang="ts">
  import { Loader2, Mail } from "lucide-svelte";
  import {
    Avatar,
    AvatarFallback,
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
  import type { UserProfile } from "$lib/api";
  import { getProfileInitials } from "$lib/domain/settings";

  let {
    profile = null,
    loading = false,
    saving = false,
    firstNameDraft = $bindable(""),
    lastNameDraft = $bindable(""),
    onSave,
  } = $props<{
    profile?: UserProfile | null;
    loading?: boolean;
    saving?: boolean;
    firstNameDraft?: string;
    lastNameDraft?: string;
    onSave: () => Promise<void> | void;
  }>();
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
          <Avatar class="size-20">
            <AvatarFallback class="text-xl font-semibold">
              {getProfileInitials(profile?.first_name, profile?.last_name, profile?.email)}
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
