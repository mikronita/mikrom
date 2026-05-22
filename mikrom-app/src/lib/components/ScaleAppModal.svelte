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
  import { Loader2, Scale } from "lucide-svelte";

  export let open = false;
  export let app: AppInfo;

  let loading = false;
  let config = {
    desired_replicas: app.desired_replicas,
    min_replicas: 0, // Mandatory scale-to-zero
    max_replicas: app.max_replicas,
    autoscaling_enabled: app.autoscaling_enabled,
    cpu_threshold: app.cpu_threshold,
    mem_threshold: app.mem_threshold,
  };

  async function handleSave() {
    const token = getToken();
    if (!token) return;

    if (!config.autoscaling_enabled) {
      config.min_replicas = 0;
      config.max_replicas = config.desired_replicas;
    }

    loading = true;
    try {
      const result = await scaleApp(token, app.name, {
        ...config,
        min_replicas: 0, // Ensure it's 0
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
      <Switch bind:checked={config.autoscaling_enabled} />
    </div>
{#if !config.autoscaling_enabled}
  <Field label="Desired Replicas" description="Fixed number of instances to keep running (max 3).">
    <Input type="number" bind:value={config.desired_replicas} min={0} max={3} />
  </Field>
{:else}
  <FieldSet class="grid grid-cols-1 gap-4 space-y-0">
    <Field label="Max Replicas" description="Maximum number of instances to scale up to (max 3).">
      <Input type="number" bind:value={config.max_replicas} min={1} max={3} />
    </Field>
  </FieldSet>

  <FieldSet class="rounded-lg border border-border p-4 bg-muted/10">
        <FieldLegend>Thresholds</FieldLegend>
        <div class="grid grid-cols-2 gap-4">
          <Field label="CPU Threshold (%)" description="Scale up when above this.">
            <Input type="number" bind:value={config.cpu_threshold} min={10} max={95} />
          </Field>
          <Field label="Memory Threshold (%)" description="Scale up when above this.">
            <Input type="number" bind:value={config.mem_threshold} min={10} max={95} />
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
