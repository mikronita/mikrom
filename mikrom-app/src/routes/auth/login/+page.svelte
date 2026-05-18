<script lang="ts">
  import { goto } from "$app/navigation";
  import { tick } from "svelte";
  import { onMount } from "svelte";
  import { Loader2, Box } from "lucide-svelte";
  import Card from "$lib/components/Card.svelte";
  import Field from "$lib/components/Field.svelte";
  import Input from "$lib/components/Input.svelte";
  import { login } from "$lib/api";
  import { setToken } from "$lib/auth";
  import { toast } from "$lib/toast";

  let email = "";
  let password = "";
  let loading = false;

  onMount(() => {
    const registered = new URLSearchParams(window.location.search).get("registered") === "true";
    if (registered) {
      toast.success("Account created! You can now sign in.");
    }
  });

  async function handleSubmit(event: SubmitEvent) {
    event.preventDefault();

    if (!email || !password) {
      toast.error("Email and password are required");
      return;
    }

    loading = true;
    const result = await login({ email, password });
    loading = false;

    if (result.error) {
      toast.error(result.error);
      return;
    }

    if (result.data) {
      setToken(result.data.token);
      await tick();
      await goto("/");
    }
  }
</script>

<svelte:head>
  <title>Mikrom - Login</title>
</svelte:head>

<div class="flex min-h-screen flex-col bg-background px-4 py-10">
  <div class="mx-auto flex w-full max-w-md flex-1 flex-col items-center justify-center gap-6">
    <div class="flex flex-col items-center gap-3 text-center">
      <div class="flex size-10 items-center justify-center rounded-full border border-border bg-card text-foreground shadow-sm">
        <Box class="size-5" />
      </div>
      <div class="flex flex-col gap-1">
        <h1 class="text-2xl font-semibold tracking-tight">Sign in to Mikrom</h1>
        <p class="text-sm text-muted-foreground">Use your account to manage applications and microVMs.</p>
      </div>
    </div>

    <Card class="w-full max-w-md">
      <div class="flex flex-col items-center gap-2.5 border-b border-border px-5 py-5 text-center">
        <div class="mb-2 flex justify-center">
          <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
            <Box class="size-4" />
          </div>
        </div>
        <div class="text-2xl font-semibold tracking-tight">Welcome back</div>
        <div class="text-sm text-muted-foreground">Enter your credentials to access your dashboard</div>
      </div>
      <div class="p-5">
        <form class="flex flex-col gap-4" on:submit|preventDefault={handleSubmit}>
          <Field label="Email address" forId="email">
            <Input id="email" type="email" bind:value={email} placeholder="name@example.com" required disabled={loading} />
          </Field>

          <div class="flex flex-col gap-1.5">
            <div class="flex items-center justify-between">
              <label for="password" class="text-sm font-medium">Password</label>
              <button type="button" class="text-xs text-muted-foreground transition-colors hover:text-foreground">Forgot password?</button>
            </div>
            <Input id="password" type="password" bind:value={password} placeholder="••••••••" required disabled={loading} />
          </div>

          <div class="flex flex-col gap-4 pt-2">
            <button
              type="submit"
              disabled={loading}
              class="inline-flex h-9 w-full items-center justify-center gap-2 whitespace-nowrap rounded-md border border-transparent bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
            >
              {#if loading}
                <Loader2 class="size-4 animate-spin" />
                Signing in...
              {:else}
                Sign In
              {/if}
            </button>
            <div class="text-center text-sm text-muted-foreground">
              Don&apos;t have an account?
              <a href="/auth/register" class="font-semibold text-foreground hover:underline"> Create one for free</a>
            </div>
          </div>
        </form>
      </div>
    </Card>

    <p class="max-w-sm text-center text-xs leading-5 text-muted-foreground">
      Protected by your workspace credentials. By continuing, you agree to Mikrom&apos;s terms and privacy policy.
    </p>
  </div>
</div>
