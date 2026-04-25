"use client";

import { useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
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
import { Label } from "@/components/ui/label";
import { useCreateApp } from "@/lib/hooks/use-apps";
import { toast } from "sonner";

interface CreateAppModalProps {
  onClose: () => void;
}

export function CreateAppModal({ onClose }: CreateAppModalProps) {
  const router = useRouter();
  const [name, setName] = useState("");
  const [gitUrl, setGitUrl] = useState("");
  const createAppMutation = useCreateApp();

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();

    createAppMutation.mutate({ name, git_url: gitUrl }, {
      onSuccess: (data) => {
        toast.success(`App ${name} created successfully`);
        onClose();
        if (data?.id) {
          router.push(`/apps/${data.id}`);
        }
      },
      onError: (error) => {
        toast.error(error instanceof Error ? error.message : "Failed to create app");
      }
    });
  };

  return (
    <Dialog open={true} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-[425px]">
        <DialogHeader>
          <DialogTitle>Create New Application</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-6 pt-4">
          <div className="space-y-4">
            <div className="grid w-full items-center gap-1.5">
              <Label htmlFor="app_name">App Name</Label>
              <Input
                id="app_name"
                required
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="my-cool-project"
              />
            </div>

            <div className="grid w-full items-center gap-1.5">
              <Label htmlFor="git_url">Git Repository URL</Label>
              <Input
                id="git_url"
                required
                value={gitUrl}
                onChange={(e) => setGitUrl(e.target.value)}
                placeholder="https://github.com/user/repo"
              />
              <p className="text-[10px] text-muted-foreground italic">
                Mikrom will automatically detect your Dockerfile or build settings.
              </p>
            </div>
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={createAppMutation.isPending}>
              {createAppMutation.isPending ? (
                <>
                  <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                  Creating...
                </>
              ) : (
                "Create App"
              )}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
