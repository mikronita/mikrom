<script lang="ts">
  import { goto } from "$app/navigation";
  import { tick } from "svelte";
    import Loader2 from "lucide-svelte/icons/loader-2";
  import UserPlus from "lucide-svelte/icons/user-plus";
  import { Card, Field, Input, Button } from "$lib/components";
  import { register } from "$lib/api";
  import { toast } from "$lib/toast";

  let email = "";
  let password = "";
  let confirmPassword = "";
  let loading = false;

  async function handleSubmit(event: SubmitEvent) {
    event.preventDefault();

    if (!email || !password) {
      toast.error("Email and password are required");
      return;
    }

    if (password.length < 8) {
      toast.error("Password must be at least 8 characters");
      return;
    }

    if (password !== confirmPassword) {
      toast.error("Passwords do not match");
      return;
    }

    loading = true;
    const result = await register({ email, password });
    loading = false;

    if (result.error) {
      toast.error(result.error);
      return;
    }

    if (result.data) {
      await tick();
      await goto("/auth/login?registered=true");
    }
  }
</script>

<svelte:head>
  <title>Mikrom - Register</title>
</svelte:head>

<div class="flex min-h-screen flex-col bg-background px-4 py-10">
  <div class="mx-auto flex w-full max-w-md flex-1 flex-col items-center justify-center gap-6">
    <div class="flex flex-col items-center gap-3 text-center">
      <div class="flex size-10 items-center justify-center rounded-md border border-border bg-card text-foreground shadow-xs">
        <UserPlus class="size-5" />
      </div>
      <div class="flex flex-col gap-1">
        <h1 class="text-2xl font-semibold tracking-tight">Create your Mikrom account</h1>
        <p class="text-sm text-muted-foreground">Set up access to deploy and manage your applications across projects.</p>
      </div>
    </div>

    <Card class="w-full max-w-md">
      <div class="p-7">
        <form class="flex flex-col gap-5" onsubmit={handleSubmit}>
          <Field label="Email address" forId="email">
            <Input id="email" type="email" bind:value={email} placeholder="name@example.com" required disabled={loading} />
          </Field>

          <Field label="Password" forId="password">
            <Input id="password" type="password" bind:value={password} placeholder="At least 8 characters" required disabled={loading} />
          </Field>

          <Field label="Confirm Password" forId="confirmPassword">
            <Input id="confirmPassword" type="password" bind:value={confirmPassword} placeholder="Repeat your password" required disabled={loading} />
          </Field>

          <div class="flex flex-col gap-4 pt-2">
            <Button
              type="submit"
              disabled={loading}
              class="w-full"
            >
              {#if loading}
                <Loader2 class="size-4 animate-spin" />
                Creating account...
              {:else}
                Create account
              {/if}
            </Button>
            <div class="text-center text-sm text-muted-foreground">
              Already have an account?
              <a href="/auth/login" class="font-medium text-foreground hover:underline"> Sign in</a>
            </div>
          </div>
        </form>
      </div>
    </Card>

    <p class="max-w-sm text-center text-xs leading-5 text-muted-foreground">
      By continuing, you agree to Mikrom&apos;s terms and privacy policy.
    </p>
  </div>
</div>
