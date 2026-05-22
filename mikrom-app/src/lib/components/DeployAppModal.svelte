<script lang="ts">
  import { Loader2, Rocket } from "lucide-svelte";
  import {
    DEPLOYMENT_CPU_OPTIONS,
    DEPLOYMENT_MEMORY_OPTIONS,
    DEPLOYMENT_HYPERVISOR_OPTIONS,
    deployAppVersion,
    type AppInfo,
  } from "$lib/api";
  import { getToken } from "$lib/auth";
  import Button from "$lib/components/Button.svelte";
  import Field from "$lib/components/Field.svelte";
  import Modal from "$lib/components/Modal.svelte";
  import Select from "$lib/components/Select.svelte";
  import { toast } from "$lib/toast";

  export let open = false;
  export let app: AppInfo;

  const DEFAULT_CPU = String(DEPLOYMENT_CPU_OPTIONS[0]);
  const DEFAULT_MEMORY = String(DEPLOYMENT_MEMORY_OPTIONS[0].value);
  const DEFAULT_HYPERVISOR = String(DEPLOYMENT_HYPERVISOR_OPTIONS[0].value);

  let loading = false;
  let selectedCpu = DEFAULT_CPU;
  let selectedMemory = DEFAULT_MEMORY;
  let selectedHypervisor = DEFAULT_HYPERVISOR;

  function resetForm() {
    selectedCpu = DEFAULT_CPU;
    selectedMemory = DEFAULT_MEMORY;
    selectedHypervisor = DEFAULT_HYPERVISOR;
  }

  $: if (open) {
    resetForm();
  }

  function close() {
    resetForm();
    open = false;
  }

  async function handleDeploy() {
    const token = getToken();
    if (!token || !app) return;

    loading = true;
    try {
      const result = await deployAppVersion(token, app.name, {
        vcpus: Number(selectedCpu),
        memory_mib: Number(selectedMemory),
        hypervisor: selectedHypervisor || undefined,
      });

      if (result.error) {
        toast.error(result.error);
        return;
      }

      toast.success(`Deployment for ${app.name} initiated`);
      close();
    } finally {
      loading = false;
    }
  }
</script>

<Modal
  bind:open
  title={`Deploy ${app.name}`}
  description="Choose a CPU and RAM preset for this deployment."
  width="max-w-[440px]"
  on:close={close}
>
  <form class="flex flex-col gap-6 pt-2" on:submit|preventDefault={handleDeploy}>
    <div class="rounded-lg border border-border bg-muted/30 p-4">
      <p class="text-sm font-medium">Resource presets</p>
      <p class="mt-1 text-xs text-muted-foreground">Choose one CPU and one RAM preset.</p>
    </div>

    <Field label="CPU" forId="deploy_cpu" description="Choose how many vCPUs this deployment gets.">
      <Select id="deploy_cpu" bind:value={selectedCpu}>
        {#each DEPLOYMENT_CPU_OPTIONS as cpu}
          <option value={cpu.toString()}>{cpu} vCPU</option>
        {/each}
      </Select>
    </Field>

    <Field label="RAM" forId="deploy_memory" description="Choose the memory preset for this deployment.">
      <Select id="deploy_memory" bind:value={selectedMemory}>
        {#each DEPLOYMENT_MEMORY_OPTIONS as memory}
          <option value={memory.value.toString()}>{memory.label}</option>
        {/each}
      </Select>
    </Field>

    <Field
      label="Hypervisor"
      forId="deploy_hypervisor"
      description="Select the virtual machine monitor for this deployment."
    >
      <Select id="deploy_hypervisor" bind:value={selectedHypervisor}>
        {#each DEPLOYMENT_HYPERVISOR_OPTIONS as opt}
          <option value={opt.value}>{opt.label}</option>
        {/each}
      </Select>
    </Field>

    <div class="flex justify-end gap-3 pt-2">
      <Button variant="outline" type="button" onclick={close} disabled={loading}>Cancel</Button>
      <Button type="submit" disabled={loading}>
        {#if loading}
          <Loader2 class="size-4 animate-spin" />
        {:else}
          <Rocket class="size-4" />
        {/if}
        Deploy
      </Button>
    </div>
  </form>
</Modal>
