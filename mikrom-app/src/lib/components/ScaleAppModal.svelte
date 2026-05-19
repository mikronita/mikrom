<script lang="ts">
  import { scaleApp, type AppInfo } from "$lib/api";
  import { getToken } from "$lib/auth";
  import Button from "$lib/components/Button.svelte";
  import Field from "$lib/components/Field.svelte";
  import FieldGroup from "$lib/components/FieldGroup.svelte";
  import FieldSet from "$lib/components/FieldSet.svelte";
  import FieldLegend from "$lib/components/FieldLegend.svelte";
  import Input from "$lib/components/Input.svelte";
  import Modal from "$lib/components/Modal.svelte";
  import Switch from "$lib/components/Switch.svelte";
  import { toast } from "$lib/toast";
  import { refreshApps } from "$lib/stores/apps";
  import { Loader2, Scale } from "lucide-svelte";

  export let open = false;
  export let app: AppInfo;

  let loading = false;
  let config = {
    desired_replicas: app.desired_replicas,
    min_replicas: app.min_replicas,
    max_replicas: app.max_replicas,
    autoscaling_enabled: app.autoscaling_enabled,
    cpu_threshold: app.cpu_threshold,
    mem_threshold: app.mem_threshold,
  };

  async function handleSave() {
    const token = getToken();
    if (!token) return;
    loading = true;
    try {
      const result = await scaleApp(token, app.name, config);
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
  <FieldGroup className="pt-4">
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
  <FieldSet className="grid grid-cols-2 gap-4 space-y-0">
    <Field label="Min Replicas">
      <Input type="number" bind:value={config.min_replicas} min={1} max={config.max_replicas} />
    </Field>
    <Field label="Max Replicas">
      <Input type="number" bind:value={config.max_replicas} min={config.min_replicas} max={3} />
    </Field>
  </FieldSet>

  <FieldSet className="rounded-lg border border-border p-4 bg-muted/10">
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
