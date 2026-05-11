"use client";

import { useState, type FormEvent } from "react";
import { Loader2 } from "lucide-react";
import { 
  Dialog, 
  DialogContent, 
  DialogHeader, 
  DialogTitle, 
  DialogFooter 
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { useDeployApp } from "@/lib/hooks/use-vms";
import { useRouter } from "next/navigation";
import { DeployRequest } from "@/lib/api";
import { toast } from "sonner";

interface DeployForm {
  app_name: string;
  image: string;
  vcpus: string;
  memory_mib: string;
  disk_mib: string;
}

const EMPTY_FORM: DeployForm = {
  app_name: "",
  image: "",
  vcpus: "",
  memory_mib: "",
  disk_mib: "",
};

interface DeployModalProps {
  onClose: () => void;
}

export function DeployModal({ onClose }: DeployModalProps) {
  const router = useRouter();
  const [form, setForm] = useState<DeployForm>(EMPTY_FORM);
  const deployMutation = useDeployApp();

  const handleDeploySubmit = async (e: FormEvent) => {
    e.preventDefault();

    const payload: DeployRequest = {
      app_name: form.app_name,
      image: form.image,
    };
    if (form.vcpus) payload.vcpus = parseInt(form.vcpus, 10);
    if (form.memory_mib) payload.memory_mib = parseInt(form.memory_mib, 10);
    if (form.disk_mib) payload.disk_mib = parseInt(form.disk_mib, 10);

    deployMutation.mutate(payload, {
      onSuccess: () => {
        toast.success(`App ${form.app_name} deployment initiated`);
        onClose();
        router.push(`/apps/${form.app_name}`);
      },
      onError: (error) => {
        toast.error(error instanceof Error ? error.message : "Deploy failed");
      }
    });
  };

  return (
    <Dialog open={true} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-[425px]" aria-describedby={undefined}>
        <DialogHeader>
          <DialogTitle>Deploy New App</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleDeploySubmit} className="flex flex-col gap-6 pt-2">
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="app_name">App Name</FieldLabel>
              <Input
                id="app_name"
                required
                value={form.app_name}
                onChange={(e) => setForm((f) => ({ ...f, app_name: e.target.value }))}
                placeholder="my-micro-service"
              />
            </Field>

            <Field>
              <FieldLabel htmlFor="image">Docker Image / RootFS</FieldLabel>
              <Input
                id="image"
                required
                value={form.image}
                onChange={(e) => setForm((f) => ({ ...f, image: e.target.value }))}
                placeholder="e.g. nginx:alpine"
              />
            </Field>

            <div className="grid gap-4 sm:grid-cols-3">
              <Field>
                <FieldLabel htmlFor="vcpus">vCPUs</FieldLabel>
                <Input
                  id="vcpus"
                  type="number"
                  min="1"
                  value={form.vcpus}
                  onChange={(e) => setForm((f) => ({ ...f, vcpus: e.target.value }))}
                  placeholder="1"
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="memory">RAM (MiB)</FieldLabel>
                <Input
                  id="memory"
                  type="number"
                  min="64"
                  value={form.memory_mib}
                  onChange={(e) => setForm((f) => ({ ...f, memory_mib: e.target.value }))}
                  placeholder="512"
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="disk">Disk (MiB)</FieldLabel>
                <Input
                  id="disk"
                  type="number"
                  min="128"
                  value={form.disk_mib}
                  onChange={(e) => setForm((f) => ({ ...f, disk_mib: e.target.value }))}
                  placeholder="1024"
                />
              </Field>
            </div>
          </FieldGroup>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={deployMutation.isPending}>
              {deployMutation.isPending ? (
                <>
                  <Loader2 data-icon="inline-start" className="animate-spin" />
                  Deploying...
                </>
              ) : (
                "Launch Instance"
              )}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
