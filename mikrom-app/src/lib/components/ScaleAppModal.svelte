<script lang="ts">
  import { scaleApp, type AppInfo } from "$lib/api";
  import { getToken } from "$lib/auth";
  import {
    Button,
    Field,
    FieldGroup,
    FieldSet,
    FieldLegend,
    Input,
    Modal,
    Switch,
  } from "$lib/components";
  import { toast } from "$lib/toast";
  import { refreshApps } from "$lib/stores/apps";
    import Loader2 from "@lucide/svelte/icons/loader-2";
  import Scale from "@lucide/svelte/icons/scale";

  let {
    open = $bindable(false),
    app,
  } = $props<{
    open?: boolean;
    app: AppInfo;
  }>();

  let loading = $state(false);
  let desiredReplicas = $state(0);
  let maxReplicas = $state(1);
  let autoscalingEnabled = $state(false);
  let cpuThreshold = $state(60);
  let memThreshold = $state(60);

  $effect(() => {
    if (open) {
      desiredReplicas = app.desired_replicas;
      maxReplicas = app.max_replicas;
      autoscalingEnabled = app.autoscaling_enabled;
      cpuThreshold = app.cpu_threshold;
      memThreshold = app.mem_threshold;
    }
  });

  async function handleSave() {
    const token = getToken();
    if (!token) return;

    if (!autoscalingEnabled) {
      maxReplicas = desiredReplicas;
    }

    loading = true;
    try {
      const result = await scaleApp(token, app.name, {
        desired_replicas: desiredReplicas,
        min_replicas: 0,
        max_replicas: maxReplicas,
        autoscaling_enabled: autoscalingEnabled,
        cpu_threshold: cpuThreshold,
        mem_threshold: memThreshold,
      });
      if (result.error) {
        toast.error(result.error);
        return;
      }
      toast.success("Scaling configuration updated");
      await refreshApps();
      open = false;
    } finally {
      loading = false;
    }
  }
</script>

<Modal bind:open title={`Scaling & Reliability: ${app.name}`} description="Configure how many replicas of your application should run.">
  <FieldGroup class="pt-4">
    <div class="rounded-lg border border-blue-500/20 p-4 bg-blue-500/5 mb-4">
      <div class="flex items-start gap-3">
        <div class="mt-0.5">
          <Scale class="size-4 text-blue-500" />
        </div>
        <div class="space-y-1">
          <div class="text-sm font-medium text-blue-500">Global Scale-to-Zero Policy</div>
          <div class="text-xs text-muted-foreground leading-relaxed">
            All Mikrom applications scale to zero after 15 minutes of inactivity to save resources. Your app will automatically wake up when it receives traffic.
          </div>
        </div>
      </div>
    </div>

    <div class="flex items-center justify-between rounded-lg border border-border p-4 bg-muted/30">
      <div class="space-y-0.5">
        <div class="text-sm font-medium">Autoscaling</div>
        <div class="text-xs text-muted-foreground">Automatically adjust replicas based on resource usage.</div>
      </div>
      <Switch bind:checked={autoscalingEnabled} />
    </div>
{#if !autoscalingEnabled}
  <Field label="Desired Replicas" description="Fixed number of instances to keep running (max 3).">
    <Input type="number" bind:value={desiredReplicas} min={0} max={3} />
  </Field>
{:else}
  <FieldSet class="grid grid-cols-1 gap-4 space-y-0">
    <Field label="Max Replicas" description="Maximum number of instances to scale up to (max 3).">
      <Input type="number" bind:value={maxReplicas} min={1} max={3} />
    </Field>
  </FieldSet>

  <FieldSet class="rounded-lg border border-border p-4 bg-muted/10">
        <FieldLegend>Thresholds</FieldLegend>
        <div class="grid grid-cols-2 gap-4">
          <Field label="CPU Threshold (%)" description="Scale up when above this.">
            <Input type="number" bind:value={cpuThreshold} min={10} max={95} />
          </Field>
          <Field label="Memory Threshold (%)" description="Scale up when above this.">
            <Input type="number" bind:value={memThreshold} min={10} max={95} />
          </Field>
        </div>
      </FieldSet>
    {/if}

    <div class="flex justify-end gap-3 pt-2">
      <Button variant="outline" onclick={() => (open = false)} disabled={loading}>Cancel</Button>
      <Button onclick={handleSave} disabled={loading}>
        {#if loading}
          <Loader2 class="size-4 animate-spin" />
        {:else}
          <Scale class="size-4" />
        {/if}
        Save Configuration
      </Button>
    </div>
  </FieldGroup>
</Modal>
