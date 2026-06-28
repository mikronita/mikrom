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
  import {
    Button,
    Field,
    Input,
    Modal,
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
  } from "$lib/components";
  import { toast } from "$lib/toast";

  let {
    open = $bindable(false),
    app,
  } = $props<{
    open?: boolean;
    app: AppInfo;
  }>();

  const DEFAULT_CPU = String(DEPLOYMENT_CPU_OPTIONS[0]);
  const DEFAULT_MEMORY = String(DEPLOYMENT_MEMORY_OPTIONS[0].value);
  const DEFAULT_HYPERVISOR = String(DEPLOYMENT_HYPERVISOR_OPTIONS[0].value);
  const DEFAULT_PORT = "8080";

  let loading = $state(false);
  let selectedCpu = $state(DEFAULT_CPU);
  let selectedMemory = $state(DEFAULT_MEMORY);
  let selectedHypervisor = $state(DEFAULT_HYPERVISOR);
  let selectedPort = $state(DEFAULT_PORT);

  function resetForm() {
    selectedCpu = DEFAULT_CPU;
    selectedMemory = DEFAULT_MEMORY;
    selectedHypervisor = DEFAULT_HYPERVISOR;
    selectedPort = DEFAULT_PORT;
  }

  $effect(() => {
    if (open) {
      resetForm();
    }
  });

  function close() {
    resetForm();
    open = false;
  }

  async function handleDeploy(event: SubmitEvent) {
    event.preventDefault();
    const token = getToken();
    if (!token || !app) return;

    loading = true;
    try {
      const result = await deployAppVersion(token, app.name, {
        vcpus: Number(selectedCpu),
        memory_mib: Number(selectedMemory),
        port: Number(selectedPort),
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
  onclose={close}
>
  <form class="flex flex-col gap-6 pt-2" onsubmit={handleDeploy}>
    <div class="rounded-lg border border-border bg-muted/30 p-4">
      <p class="text-sm font-medium">Resource presets</p>
      <p class="mt-1 text-xs text-muted-foreground">Choose one CPU and one RAM preset.</p>
    </div>

    <Field label="CPU" forId="deploy_cpu" description="Choose how many vCPUs this deployment gets.">
      <Select bind:value={selectedCpu}>
        <SelectTrigger id="deploy_cpu">
          <SelectValue placeholder="Select CPU" />
        </SelectTrigger>
        <SelectContent>
          {#each DEPLOYMENT_CPU_OPTIONS as cpu}
            <SelectItem value={cpu.toString()}>{cpu} vCPU</SelectItem>
          {/each}
        </SelectContent>
      </Select>
    </Field>

    <Field label="RAM" forId="deploy_memory" description="Choose the memory preset for this deployment.">
      <Select bind:value={selectedMemory}>
        <SelectTrigger id="deploy_memory">
          <SelectValue placeholder="Select RAM" />
        </SelectTrigger>
        <SelectContent>
          {#each DEPLOYMENT_MEMORY_OPTIONS as memory}
            <SelectItem value={memory.value.toString()}>{memory.label}</SelectItem>
          {/each}
        </SelectContent>
      </Select>
    </Field>

    <Field
      label="Container Port"
      forId="deploy_port"
      description="Port exposed by the container inside the microVM."
    >
      <Input
        id="deploy_port"
        bind:value={selectedPort}
        type="number"
        min="1"
        max="65535"
        step="1"
        inputmode="numeric"
        placeholder="8080"
      />
    </Field>

    <Field
      label="Hypervisor"
      forId="deploy_hypervisor"
      description="Select the virtual machine monitor for this deployment."
    >
      <Select bind:value={selectedHypervisor}>
        <SelectTrigger id="deploy_hypervisor">
          <SelectValue placeholder="Select Hypervisor" />
        </SelectTrigger>
        <SelectContent>
          {#each DEPLOYMENT_HYPERVISOR_OPTIONS as opt}
            <SelectItem value={opt.value}>{opt.label}</SelectItem>
          {/each}
        </SelectContent>
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
