"use client";

import { useState, type FormEvent } from "react";
import { Plus, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/Card";
import { Input } from "@/components/ui/Input";
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
      onSuccess: (data) => {
        toast.success(`App ${form.app_name} deployment initiated`);
        onClose();
        if (data?.job_id) {
          router.push(`/dashboard/vms/${data.job_id}`);
        }
      },
      onError: (error) => {
        toast.error(error instanceof Error ? error.message : "Deploy failed");
      }
    });
  };

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4 animate-in fade-in duration-200">
      <Card className="w-full max-w-md shadow-2xl animate-in zoom-in-95 duration-200">
        <CardHeader className="border-b border-zinc-100 dark:border-zinc-800">
          <div className="flex items-center justify-between">
            <CardTitle>Deploy New App</CardTitle>
            <Button variant="ghost" size="icon" className="h-8 w-8" onClick={onClose}>
              <Plus className="w-4 h-4 rotate-45" />
            </Button>
          </div>
          <CardDescription>
            Configure and launch a new virtual instance.
          </CardDescription>
        </CardHeader>

        <form onSubmit={handleDeploySubmit}>
          <CardContent className="space-y-4 pt-6">
            <div className="space-y-2">
              <label className="text-sm font-medium leading-none">
                App Name
              </label>
              <Input
                required
                value={form.app_name}
                onChange={(e) => setForm((f) => ({ ...f, app_name: e.target.value }))}
                placeholder="my-micro-service"
              />
            </div>

            <div className="space-y-2">
              <label className="text-sm font-medium leading-none">
                Docker Image / RootFS
              </label>
              <Input
                required
                value={form.image}
                onChange={(e) => setForm((f) => ({ ...f, image: e.target.value }))}
                placeholder="e.g. nginx:alpine"
              />
            </div>

            <div className="grid grid-cols-3 gap-4 pt-2">
              <div className="space-y-2">
                <label className="text-[11px] font-bold uppercase text-zinc-500">vCPUs</label>
                <Input
                  type="number"
                  min="1"
                  value={form.vcpus}
                  onChange={(e) => setForm((f) => ({ ...f, vcpus: e.target.value }))}
                  placeholder="1"
                />
              </div>
              <div className="space-y-2">
                <label className="text-[11px] font-bold uppercase text-zinc-500">RAM (MiB)</label>
                <Input
                  type="number"
                  min="64"
                  value={form.memory_mib}
                  onChange={(e) => setForm((f) => ({ ...f, memory_mib: e.target.value }))}
                  placeholder="512"
                />
              </div>
              <div className="space-y-2">
                <label className="text-[11px] font-bold uppercase text-zinc-500">Disk (MiB)</label>
                <Input
                  type="number"
                  min="128"
                  value={form.disk_mib}
                  onChange={(e) => setForm((f) => ({ ...f, disk_mib: e.target.value }))}
                  placeholder="1024"
                />
              </div>
            </div>
          </CardContent>

          <div className="p-6 pt-0 flex justify-end gap-3">
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={deployMutation.isPending}>
              {deployMutation.isPending ? (
                <>
                  <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                  Deploying...
                </>
              ) : (
                "Launch Instance"
              )}
            </Button>
          </div>
        </form>
      </Card>
    </div>
  );
}
