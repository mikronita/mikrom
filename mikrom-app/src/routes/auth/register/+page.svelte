<script lang="ts">
  import { goto } from "$app/navigation";
  import { tick, onMount } from "svelte";
  import Loader2 from "@lucide/svelte/icons/loader-2";
  import UserPlus from "@lucide/svelte/icons/user-plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { Card, Field, Input, Button } from "$lib/components";
  import { register, getCaptcha } from "$lib/api";
  import { toast } from "$lib/toast";

  let email = "";
  let password = "";
  let confirmPassword = "";
  let loading = false;

  let captchaId = "";
  let captchaImage = "";
  let captchaAnswer = "";
  let loadingCaptcha = false;

  async function loadCaptcha() {
    loadingCaptcha = true;
    const result = await getCaptcha();
    loadingCaptcha = false;
    if (result.error) {
      toast.error(result.error);
      return;
    }
    if (result.data) {
      captchaId = result.data.captcha_id;
      captchaImage = result.data.captcha_image;
      captchaAnswer = "";
    }
  }

  onMount(() => {
    loadCaptcha();
  });

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

    if (!captchaAnswer) {
      toast.error("Please answer the captcha challenge");
      return;
    }

    loading = true;
    const result = await register({
      email,
      password,
      captcha_id: captchaId,
      captcha_answer: captchaAnswer,
    });
    loading = false;

    if (result.error) {
      toast.error(result.error);
      loadCaptcha();
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

          {#if captchaImage}
            <div class="flex flex-col gap-2">
              <div class="text-sm font-medium text-foreground">Security Check (Captcha)</div>
              <div class="flex items-center gap-3">
                <div class="relative flex items-center justify-center rounded-md border border-border bg-muted/30 p-1.5 shadow-xs overflow-hidden select-none min-w-[150px] min-h-[50px]">
                  {#if loadingCaptcha}
                    <div class="absolute inset-0 flex items-center justify-center bg-muted/60 backdrop-blur-xs">
                      <Loader2 class="size-4 animate-spin text-muted-foreground" />
                    </div>
                  {/if}
                  <img src={captchaImage} alt="Captcha challenge" class="h-10 select-none pointer-events-none" />
                </div>
                
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  onclick={loadCaptcha}
                  disabled={loadingCaptcha || loading}
                  class="size-10 cursor-pointer transition-transform duration-200 active:scale-95 animate-in fade-in"
                  aria-label="Refresh captcha"
                >
                  <RefreshCw class="size-4 text-muted-foreground" />
                </Button>
              </div>
              
              <Input
                id="captchaAnswer"
                type="text"
                bind:value={captchaAnswer}
                placeholder="Result of the operation"
                required
                disabled={loading || loadingCaptcha}
                class="w-full mt-1"
                autocomplete="off"
                autocorrect="off"
                autocapitalize="none"
              />
            </div>
          {:else}
            <div class="flex flex-col gap-2">
              <div class="h-5 w-24 animate-pulse rounded bg-muted"></div>
              <div class="flex gap-3">
                <div class="h-[50px] w-[150px] animate-pulse rounded border border-border bg-muted/30"></div>
                <div class="h-10 w-10 animate-pulse rounded border border-border bg-muted/30"></div>
              </div>
              <div class="h-10 w-full animate-pulse rounded border border-border bg-muted/30 mt-1"></div>
            </div>
          {/if}

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
