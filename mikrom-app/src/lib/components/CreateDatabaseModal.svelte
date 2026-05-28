<script lang="ts">
  import { Layers } from "lucide-svelte";
  import { 
    Modal, 
    Button, 
    Input, 
    Select, 
    SelectContent, 
    SelectItem, 
    SelectTrigger, 
    SelectValue, 
    Field, 
    FieldGroup 
  } from "$lib/components";
  import { addDatabase } from "$lib/stores/databases";
  import { toast } from "$lib/toast";

  export let open = false;
  export let onClose: (() => void) | undefined = undefined;

  let name = "";
  let version = "16";
  let plan = "shared-1-1"; // vcpus-memory
  let storage_gb = 10;

  const plans = [
    { id: "shared-1-1", label: "Shared 1 vCPU / 1GB RAM", vcpus: 1, memory_mib: 1024 },
    { id: "dedicated-2-4", label: "Dedicated 2 vCPU / 4GB RAM", vcpus: 2, memory_mib: 4096 },
    { id: "dedicated-4-8", label: "Dedicated 4 vCPU / 8GB RAM", vcpus: 4, memory_mib: 8192 },
  ];

  function close() {
    open = false;
    onClose?.();
  }

  function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    
    const selectedPlan = plans.find(p => p.id === plan);
    if (!selectedPlan) return;

    addDatabase({
      name,
      version,
      vcpus: selectedPlan.vcpus,
      memory_mib: selectedPlan.memory_mib,
      storage_gb
    });

    toast.success(`Database ${name} is being provisioned`);
    close();
  }
</script>

<Modal bind:open title="Create New Database" description="Provision a managed PostgreSQL instance." width="max-w-[450px]" onclose={close}>
  <form class="flex flex-col gap-6 pt-2" on:submit|preventDefault={handleSubmit}>
    <Field label="Database Name" forId="db_name">
      <Input id="db_name" bind:value={name} placeholder="my-production-db" required />
    </Field>

    <div class="grid grid-cols-2 gap-4">
      <Field label="PostgreSQL Version" forId="db_version">
        <Select bind:value={version}>
          <SelectTrigger id="db_version">
            <SelectValue placeholder="Select version" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="16">PostgreSQL 16</SelectItem>
            <SelectItem value="15">PostgreSQL 15</SelectItem>
            <SelectItem value="14">PostgreSQL 14</SelectItem>
          </SelectContent>
        </Select>
      </Field>

      <Field label="Storage (GB)" forId="db_storage">
        <Input id="db_storage" type="number" bind:value={storage_gb} min="1" max="1000" required />
      </Field>
    </div>

    <FieldGroup label="Compute Plan">
      <div class="flex flex-col gap-2">
        {#each plans as p}
          <label class={`flex cursor-pointer items-center justify-between rounded-md border p-3 transition-colors ${plan === p.id ? 'border-primary bg-primary/5' : 'border-border hover:bg-muted/50'}`}>
            <div class="flex items-center gap-3">
              <input type="radio" name="plan" value={p.id} bind:group={plan} class="sr-only" />
              <div class={`flex size-4 items-center justify-center rounded-full border ${plan === p.id ? 'border-primary' : 'border-muted-foreground'}`}>
                {#if plan === p.id}
                  <div class="size-2 rounded-full bg-primary"></div>
                {/if}
              </div>
              <div class="flex flex-col">
                <span class="text-sm font-medium">{p.label}</span>
              </div>
            </div>
            <Layers class={`size-4 ${plan === p.id ? 'text-primary' : 'text-muted-foreground'}`} />
          </label>
        {/each}
      </div>
    </FieldGroup>

    <div class="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
      <Button variant="outline" type="button" onclick={close}>Cancel</Button>
      <Button type="submit">Provision Database</Button>
    </div>
  </form>
</Modal>
