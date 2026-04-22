"use client";

import { useState, type FormEvent } from "react";
import { Loader2 } from "lucide-react";
import { Button, Modal, ModalHeader, ModalBody, ModalFooter, Label, TextInput } from "flowbite-react";
import { useCreateApp } from "@/lib/hooks/use-apps";
import { toast } from "sonner";

interface CreateAppModalProps {
  onClose: () => void;
}

export function CreateAppModal({ onClose }: CreateAppModalProps) {
  const [name, setName] = useState("");
  const [gitUrl, setGitUrl] = useState("");
  const createAppMutation = useCreateApp();

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();

    createAppMutation.mutate({ name, git_url: gitUrl }, {
      onSuccess: () => {
        toast.success(`App ${name} created successfully`);
        onClose();
      },
      onError: (error) => {
        toast.error(error instanceof Error ? error.message : "Failed to create app");
      }
    });
  };

  return (
    <Modal show={true} onClose={onClose} size="md">
      <ModalHeader>Create New Application</ModalHeader>
      <form onSubmit={handleSubmit}>
        <ModalBody>
          <div className="space-y-6">
            <div>
              <div className="mb-2 block">
                <Label htmlFor="app_name">App Name</Label>
              </div>
              <TextInput
                id="app_name"
                required
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="my-cool-project"
              />
            </div>

            <div>
              <div className="mb-2 block">
                <Label htmlFor="git_url">Git Repository URL</Label>
              </div>
              <TextInput
                id="git_url"
                required
                value={gitUrl}
                onChange={(e) => setGitUrl(e.target.value)}
                placeholder="https://github.com/user/repo"
              />
              <p className="text-[10px] text-gray-500 mt-1 italic">
                Mikrom will automatically detect your Dockerfile or build settings.
              </p>
            </div>
          </div>
        </ModalBody>
        <ModalFooter className="justify-end">
          <Button color="gray" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" color="dark" disabled={createAppMutation.isPending}>
            {createAppMutation.isPending ? (
              <>
                <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                Creating...
              </>
            ) : (
              "Create App"
            )}
          </Button>
        </ModalFooter>
      </form>
    </Modal>
  );
}
