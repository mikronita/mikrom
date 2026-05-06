"use client";

import { useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import { Loader2 } from "lucide-react";
import { FaGithub, FaGlobe } from "react-icons/fa";
import { 
  Dialog, 
  DialogContent, 
  DialogHeader, 
  DialogTitle, 
  DialogFooter 
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Field, FieldLabel, FieldGroup, FieldDescription } from "@/components/ui/field";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { 
  Select, 
  SelectContent, 
  SelectGroup, 
  SelectItem, 
  SelectTrigger, 
  SelectValue 
} from "@/components/ui/select";
import { useCreateApp, useGithubRepos } from "@/lib/hooks/use-apps";
import { toast } from "sonner";
import { getGithubInstallUrl } from "@/lib/api";
import { getToken } from "@/lib/auth";

interface CreateAppModalProps {
  onClose: () => void;
}

export function CreateAppModal({ onClose }: CreateAppModalProps) {
  const router = useRouter();
  const [name, setName] = useState("");
  const [gitUrl, setGitUrl] = useState("");
  const [activeTab, setActiveTab] = useState("manual");
  const [selectedRepoId, setSelectedRepoId] = useState<string>("");
  
  const createAppMutation = useCreateApp();
  const { data: githubRepos, isLoading: isLoadingRepos } = useGithubRepos();

  const selectedRepo = githubRepos?.find(r => r.id.toString() === selectedRepoId);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();

    const payload = activeTab === "github" && selectedRepo 
      ? { 
          name, 
          git_url: selectedRepo.html_url,
          github_installation_id: selectedRepo.installation_id,
          github_repo_id: selectedRepo.id,
          github_repo_full_name: selectedRepo.full_name
        }
      : { name, git_url: gitUrl };

    // Note: We need the installation_id. Let's assume for now the API can figure it out from repo_id 
    // or we should have fetched it with the repos. 
    // In our listGithubRepos, we could return the installation_id per repo or just repo_id.
    // Actually, I'll update the API listRepos to include installation_id in GithubRepo.

    createAppMutation.mutate(payload, {
      onSuccess: (data) => {
        toast.success(`App ${name} created successfully`);
        onClose();
        if (data?.name) {
          router.push(`/apps/${encodeURIComponent(data.name)}`);
        }
      },
      onError: (error) => {
        toast.error(error instanceof Error ? error.message : "Failed to create app");
      }
    });
  };

  const handleRepoChange = (id: string) => {
    setSelectedRepoId(id);
    const repo = githubRepos?.find(r => r.id.toString() === id);
    if (repo) {
      if (!name) setName(repo.name);
    }
  };

  const handleConnectGithub = async () => {
    const t = getToken();
    if (!t) {
      toast.error("You must be logged in to connect GitHub");
      return;
    }

    const toastId = toast.loading("Redirecting to GitHub...");
    try {
      const { data, error } = await getGithubInstallUrl(t);
      if (error) {
        toast.dismiss(toastId);
        toast.error(error);
        return;
      }
      if (data?.url) {
        window.location.href = data.url;
      } else {
        toast.dismiss(toastId);
        toast.error("Failed to get installation URL");
      }
    } catch (err) {
      toast.dismiss(toastId);
      toast.error("Failed to start GitHub installation");
    }
  };

  return (
    <Dialog open={true} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-[425px]" aria-describedby={undefined}>
        <DialogHeader>
          <DialogTitle>Create New Application</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-6 pt-4">
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="app_name">App Name</FieldLabel>
              <Input
                id="app_name"
                required
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="my-cool-project"
              />
            </Field>

            <Tabs value={activeTab} onValueChange={setActiveTab} className="w-full">
              <TabsList className="grid w-full grid-cols-2">
                <TabsTrigger value="manual">
                  <FaGlobe className="size-4 mr-2" />
                  Manual URL
                </TabsTrigger>
                <TabsTrigger value="github">
                  <FaGithub className="size-4 mr-2" />
                  GitHub
                </TabsTrigger>
              </TabsList>
              
              <TabsContent value="manual" className="pt-4">
                <Field>
                  <FieldLabel htmlFor="git_url">Git Repository URL</FieldLabel>
                  <Input
                    id="git_url"
                    required={activeTab === "manual"}
                    value={gitUrl}
                    onChange={(e) => setGitUrl(e.target.value)}
                    placeholder="https://github.com/user/repo"
                  />
                  <FieldDescription className="text-[10px] italic">
                    Public repositories only. For private ones, use the GitHub integration.
                  </FieldDescription>
                </Field>
              </TabsContent>

              <TabsContent value="github" className="pt-4">
                <Field>
                  <FieldLabel htmlFor="github_repo">Select Repository</FieldLabel>
                  {isLoadingRepos ? (
                    <div className="flex items-center justify-center p-4 border rounded-md">
                      <Loader2 className="size-4 animate-spin mr-2" />
                      Loading repositories...
                    </div>
                  ) : githubRepos && githubRepos.length > 0 ? (
                    <Select value={selectedRepoId} onValueChange={handleRepoChange}>
                      <SelectTrigger>
                        <SelectValue placeholder="Select a repository" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectGroup>
                          {githubRepos.map(repo => (
                            <SelectItem key={repo.id} value={repo.id.toString()}>
                              <div className="flex items-center gap-2">
                                {repo.private && <Loader2 className="size-3 text-muted-foreground" /> /* Just as a placeholder for lock icon */}
                                <span>{repo.full_name}</span>
                              </div>
                            </SelectItem>
                          ))}
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                  ) : (
                    <div className="text-center p-6 border rounded-md space-y-4">
                      <p className="text-sm text-muted-foreground">No GitHub accounts connected.</p>
                      <Button 
                        size="sm" 
                        variant="outline"
                        type="button"
                        onClick={handleConnectGithub}
                      >
                        Connect GitHub
                      </Button>
                    </div>
                  )}
                </Field>
              </TabsContent>
            </Tabs>
          </FieldGroup>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={createAppMutation.isPending || (activeTab === "github" && !selectedRepoId)}>
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
