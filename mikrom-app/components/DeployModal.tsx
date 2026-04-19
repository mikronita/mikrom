"use client";

import { useState, type FormEvent } from "react";
import { Loader2 } from "lucide-react";
import { Button, Modal, ModalHeader, ModalBody, ModalFooter, Label, TextInput } from "flowbite-react";
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
          router.push(`/vms/${data.job_id}`);
        }
      },
      onError: (error) => {
        toast.error(error instanceof Error ? error.message : "Deploy failed");
      }
    });
  };

  return (
    <Modal show={true} onClose={onClose} size="md">
      <ModalHeader>Deploy New App</ModalHeader>
      <form onSubmit={handleDeploySubmit}>
        <ModalBody>
          <div className="space-y-6">
            <div>
              <div className="mb-2 block">
                <Label htmlFor="app_name">App Name</Label>
              </div>
              <TextInput
                id="app_name"
                required
                value={form.app_name}
                onChange={(e) => setForm((f) => ({ ...f, app_name: e.target.value }))}
                placeholder="my-micro-service"
              />
            </div>

            <div>
              <div className="mb-2 block">
                <Label htmlFor="image">Docker Image / RootFS</Label>
              </div>
              <TextInput
                id="image"
                required
                value={form.image}
                onChange={(e) => setForm((f) => ({ ...f, image: e.target.value }))}
                placeholder="e.g. nginx:alpine"
              />
            </div>

            <div className="grid grid-cols-3 gap-4">
              <div>
                <div className="mb-2 block">
                  <Label htmlFor="vcpus" className="text-[10px] uppercase">vCPUs</Label>
                </div>
                <TextInput
                  id="vcpus"
                  type="number"
                  min="1"
                  value={form.vcpus}
                  onChange={(e) => setForm((f) => ({ ...f, vcpus: e.target.value }))}
                  placeholder="1"
                />
              </div>
              <div>
                <div className="mb-2 block">
                  <Label htmlFor="memory" className="text-[10px] uppercase">RAM (MiB)</Label>
                </div>
                <TextInput
                  id="memory"
                  type="number"
                  min="64"
                  value={form.memory_mib}
                  onChange={(e) => setForm((f) => ({ ...f, memory_mib: e.target.value }))}
                  placeholder="512"
                />
              </div>
              <div>
                <div className="mb-2 block">
                  <Label htmlFor="disk" className="text-[10px] uppercase">Disk (MiB)</Label>
                </div>
                <TextInput
                  id="disk"
                  type="number"
                  min="128"
                  value={form.disk_mib}
                  onChange={(e) => setForm((f) => ({ ...f, disk_mib: e.target.value }))}
                  placeholder="1024"
                />
              </div>
            </div>
          </div>
        </ModalBody>
        <ModalFooter className="justify-end">
          <Button color="gray" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" color="dark" disabled={deployMutation.isPending}>
            {deployMutation.isPending ? (
              <>
                <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                Deploying...
              </>
            ) : (
              "Launch Instance"
            )}
          </Button>
        </ModalFooter>
      </form>
    </Modal>
  );
}
