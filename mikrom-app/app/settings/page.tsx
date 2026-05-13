"use client";

import { useState } from "react";
import {
  Bell,
  CheckCircle2,
  CloudDownload,
  CreditCard,
  KeyRound,
  Loader2,
  Mail,
  Plus,
  Puzzle,
  Settings,
  ShieldCheck,
  Trash2,
  User,
} from "lucide-react";
import { FaGithub } from "react-icons/fa";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { getGithubInstallUrl, getUserProfile, listGithubAccounts, updateUserProfile } from "@/lib/api";
import { getToken } from "@/lib/auth";

const settingsTabs = [
  { value: "profile", label: "Profile", icon: User },
  { value: "security", label: "Security", icon: KeyRound },
  { value: "api", label: "API access", icon: CloudDownload },
  { value: "billing", label: "Billing", icon: CreditCard },
  { value: "integrations", label: "Integrations", icon: Puzzle },
  { value: "notifications", label: "Notifications", icon: Bell },
];

export default function SettingsPage() {
  const [emailNotifications, setEmailNotifications] = useState(true);
  const [marketingEmails, setMarketingNotifications] = useState(false);
  const [firstNameDraft, setFirstNameDraft] = useState<string | null>(null);
  const [lastNameDraft, setLastNameDraft] = useState<string | null>(null);

  const queryClient = useQueryClient();
  const token = getToken();

  const { data: profile, isLoading } = useQuery({
    queryKey: ["profile"],
    queryFn: () => getUserProfile(token!).then((res) => {
      if (res.error) throw new Error(res.error);
      return res.data;
    }),
    enabled: !!token,
  });

  const { data: githubAccounts, isLoading: isLoadingGithub } = useQuery({
    queryKey: ["github-accounts"],
    queryFn: () => listGithubAccounts(token!).then((res) => {
      if (res.error) throw new Error(res.error);
      return res.data;
    }),
    enabled: !!token,
  });

  const initials = profile
    ? `${profile.first_name?.[0] || ""}${profile.last_name?.[0] || ""}`.toUpperCase() ||
      profile.email?.[0]?.toUpperCase() ||
      "U"
    : "U";

  const updateMutation = useMutation({
    mutationFn: (data: { first_name: string; last_name: string }) =>
      updateUserProfile(token!, data).then((res) => {
        if (res.error) throw new Error(res.error);
        return res.data;
      }),
    onSuccess: (data) => {
      if (data) {
        setFirstNameDraft(null);
        setLastNameDraft(null);
      }
      queryClient.invalidateQueries({ queryKey: ["profile"] });
      toast.success("Profile updated successfully");
    },
    onError: (error: Error) => {
      toast.error(error.message || "Failed to update profile");
    },
  });

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
    } catch {
      toast.dismiss(toastId);
      toast.error("Failed to start GitHub installation");
    }
  };

  const handleSave = () => {
    updateMutation.mutate({
      first_name: firstNameDraft ?? profile?.first_name ?? "",
      last_name: lastNameDraft ?? profile?.last_name ?? "",
    });
  };

  const firstName = firstNameDraft ?? profile?.first_name ?? "";
  const lastName = lastNameDraft ?? profile?.last_name ?? "";

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="flex flex-col gap-6">
          <div className="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
            <div className="flex flex-col gap-2">
              <div className="flex items-center gap-3">
                <div className="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
                  <Settings />
                </div>
                <h1 className="text-3xl font-semibold tracking-tight">Settings</h1>
              </div>
              <p className="max-w-2xl text-sm text-muted-foreground">
                Manage your personal information, security preferences and billing.
              </p>
            </div>
          </div>

          <Tabs defaultValue="profile" className="flex w-full flex-col gap-5">
            <TabsList className="grid h-auto w-full grid-cols-2 gap-1 overflow-hidden p-1 sm:grid-cols-3 xl:grid-cols-6">
              {settingsTabs.map((item) => (
                <TabsTrigger
                  key={item.value}
                  value={item.value}
                  className="min-h-10 justify-start gap-2 px-3 sm:justify-center"
                >
                  <item.icon data-icon="inline-start" />
                  <span className="truncate">{item.label}</span>
                </TabsTrigger>
              ))}
            </TabsList>

            <TabsContent value="profile" className="m-0">
              <Card>
                <CardHeader>
                  <CardTitle>Profile</CardTitle>
                  <CardDescription>Update the public name and contact details for your account.</CardDescription>
                </CardHeader>
                <CardContent>
                  {isLoading ? (
                    <div className="flex min-h-64 items-center justify-center">
                      <Loader2 className="animate-spin text-muted-foreground" />
                    </div>
                  ) : (
                    <div className="flex flex-col gap-8">
                      <div className="flex flex-col items-start gap-5 sm:flex-row sm:items-center">
                        <Avatar className="size-20">
                          <AvatarFallback className="text-xl font-semibold">{initials}</AvatarFallback>
                        </Avatar>
                        <div className="flex flex-1 flex-col gap-3">
                          <div className="flex flex-col gap-1">
                            <h3 className="text-base font-semibold">Profile picture</h3>
                            <p className="text-sm text-muted-foreground">JPG, GIF or PNG. Max size of 800K.</p>
                          </div>
                          <div className="flex flex-wrap gap-2">
                            <Button size="sm">Upload new</Button>
                            <Button variant="outline" size="sm">Remove</Button>
                          </div>
                        </div>
                      </div>

                      <Separator />

                      <FieldGroup className="grid grid-cols-1 gap-6 md:grid-cols-2">
                        <Field>
                          <FieldLabel htmlFor="firstName">First name</FieldLabel>
                          <Input
                            id="firstName"
                            placeholder="John"
                            value={firstName}
                            onChange={(event) => setFirstNameDraft(event.target.value)}
                          />
                        </Field>
                        <Field>
                          <FieldLabel htmlFor="lastName">Last name</FieldLabel>
                          <Input
                            id="lastName"
                            placeholder="Doe"
                            value={lastName}
                            onChange={(event) => setLastNameDraft(event.target.value)}
                          />
                        </Field>
                        <Field className="md:col-span-2">
                          <FieldLabel htmlFor="email">Email address</FieldLabel>
                          <InputGroup>
                            <InputGroupAddon>
                              <Mail data-icon="inline-start" />
                            </InputGroupAddon>
                            <InputGroupInput
                              id="email"
                              type="email"
                              placeholder="john@example.com"
                              value={profile?.email || ""}
                              disabled
                            />
                          </InputGroup>
                          <FieldDescription>Email cannot be changed yet.</FieldDescription>
                        </Field>
                      </FieldGroup>
                    </div>
                  )}
                </CardContent>
                <CardFooter className="justify-end">
                  <Button onClick={handleSave} disabled={isLoading || updateMutation.isPending}>
                    {updateMutation.isPending && <Loader2 data-icon="inline-start" className="animate-spin" />}
                    Save changes
                  </Button>
                </CardFooter>
              </Card>
            </TabsContent>

            <TabsContent value="security" className="m-0">
              <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(20rem,0.8fr)]">
                <Card>
                  <CardHeader>
                    <CardTitle>Change password</CardTitle>
                    <CardDescription>Use a strong password that you do not use anywhere else.</CardDescription>
                  </CardHeader>
                  <CardContent>
                    <FieldGroup className="max-w-md">
                      <Field>
                        <FieldLabel htmlFor="currentPassword">Current password</FieldLabel>
                        <Input id="currentPassword" type="password" />
                      </Field>
                      <Field>
                        <FieldLabel htmlFor="newPassword">New password</FieldLabel>
                        <Input id="newPassword" type="password" />
                      </Field>
                    </FieldGroup>
                  </CardContent>
                  <CardFooter>
                    <Button>Update password</Button>
                  </CardFooter>
                </Card>

                <div className="flex flex-col gap-4">
                  <Card>
                    <CardHeader className="flex flex-row items-start justify-between gap-4">
                      <div className="flex flex-col gap-1.5">
                        <CardTitle>Two-factor authentication</CardTitle>
                        <CardDescription>Add an extra layer of security to your account.</CardDescription>
                      </div>
                      <Badge variant="warning">
                        <ShieldCheck data-icon="inline-start" />
                        Not enabled
                      </Badge>
                    </CardHeader>
                    <CardFooter>
                      <Button size="sm">Configure 2FA</Button>
                    </CardFooter>
                  </Card>

                  <Card>
                    <CardHeader>
                      <CardTitle className="text-destructive">Danger zone</CardTitle>
                      <CardDescription>
                        Once you delete your account, there is no going back. Please be certain.
                      </CardDescription>
                    </CardHeader>
                    <CardFooter>
                      <Button variant="destructive" size="sm">
                        <Trash2 data-icon="inline-start" />
                        Delete account
                      </Button>
                    </CardFooter>
                  </Card>
                </div>
              </div>
            </TabsContent>

            <TabsContent value="api" className="m-0">
              <Card>
                <CardHeader className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
                  <div className="flex flex-col gap-1.5">
                    <CardTitle>Personal access tokens</CardTitle>
                    <CardDescription>Use tokens to authenticate with the Mikrom CLI and API.</CardDescription>
                  </div>
                  <Button size="sm">
                    <Plus data-icon="inline-start" />
                    Create token
                  </Button>
                </CardHeader>
                <CardContent>
                  <div className="flex flex-col gap-4">
                    <div className="flex flex-col gap-4 rounded-lg border bg-muted/30 p-4 sm:flex-row sm:items-center sm:justify-between">
                      <div className="flex items-center gap-4">
                        <div className="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
                          <CheckCircle2 />
                        </div>
                        <div className="min-w-0">
                          <p className="truncate font-mono text-sm font-semibold">mikrom_pk_live_****************</p>
                          <p className="text-xs text-muted-foreground">Last used 2 hours ago - Created April 12, 2026</p>
                        </div>
                      </div>
                      <Button variant="destructive" size="sm">Revoke</Button>
                    </div>
                  </div>
                </CardContent>
              </Card>
            </TabsContent>

            <TabsContent value="billing" className="m-0">
              <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(20rem,0.85fr)]">
                <Card className="overflow-hidden">
                  <CardHeader className="border-b bg-muted/30">
                    <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
                      <div className="flex flex-col gap-2">
                        <CardDescription>Current plan</CardDescription>
                        <CardTitle className="text-2xl">Pro developer</CardTitle>
                        <p className="text-sm text-muted-foreground">$29 / month</p>
                      </div>
                      <Badge variant="secondary">Active</Badge>
                    </div>
                  </CardHeader>
                  <CardContent className="pt-6">
                    <div className="grid gap-3 sm:grid-cols-3">
                      <div className="rounded-lg border bg-background p-4">
                        <p className="text-2xl font-semibold">4</p>
                        <p className="text-sm text-muted-foreground">Active apps</p>
                      </div>
                      <div className="rounded-lg border bg-background p-4">
                        <p className="text-2xl font-semibold">120 GB</p>
                        <p className="text-sm text-muted-foreground">Bandwidth</p>
                      </div>
                      <div className="rounded-lg border bg-background p-4">
                        <p className="text-2xl font-semibold">24/7</p>
                        <p className="text-sm text-muted-foreground">Support</p>
                      </div>
                    </div>
                  </CardContent>
                  <CardFooter className="flex flex-wrap gap-2">
                    <Button size="sm">Change plan</Button>
                    <Button variant="outline" size="sm">Cancel subscription</Button>
                  </CardFooter>
                </Card>

                <Card>
                  <CardHeader>
                    <CardTitle>Payment method</CardTitle>
                    <CardDescription>Manage the card used for renewals and invoices.</CardDescription>
                  </CardHeader>
                  <CardContent>
                    <div className="flex items-center gap-4 rounded-lg border bg-muted/30 p-4">
                      <div className="flex h-8 w-12 shrink-0 items-center justify-center rounded-md border bg-background text-xs font-semibold italic text-muted-foreground">
                        VISA
                      </div>
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-semibold">Visa ending in 4242</p>
                        <p className="text-xs text-muted-foreground">Expires 12/28</p>
                      </div>
                      <Button variant="outline" size="sm">Edit</Button>
                    </div>
                  </CardContent>
                </Card>
              </div>
            </TabsContent>

            <TabsContent value="integrations" className="m-0">
              <Card>
                <CardHeader>
                  <CardTitle>Integrations</CardTitle>
                  <CardDescription>Connect source control providers and external services.</CardDescription>
                </CardHeader>
                <CardContent>
                  <FieldSet>
                    <FieldLegend>Source control</FieldLegend>
                    <FieldDescription>Connect your GitHub account to deploy private repositories.</FieldDescription>

                    <FieldGroup>
                      {isLoadingGithub ? (
                        <div className="flex justify-center p-4">
                          <Loader2 className="animate-spin text-muted-foreground" />
                        </div>
                      ) : githubAccounts && githubAccounts.length > 0 ? (
                        githubAccounts.map((account) => (
                          <div
                            key={account.id}
                            className="flex flex-col gap-4 rounded-lg border bg-muted/30 p-4 sm:flex-row sm:items-center sm:justify-between"
                          >
                            <div className="flex items-center gap-4">
                              <div className="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
                                <FaGithub />
                              </div>
                              <div className="min-w-0">
                                <p className="truncate text-sm font-semibold">@{account.github_username}</p>
                                <p className="text-xs text-muted-foreground">
                                  Connected on {new Date(account.created_at).toLocaleDateString()}
                                </p>
                              </div>
                            </div>
                            <Button variant="outline" size="sm" asChild>
                              <a
                                href={`https://github.com/settings/installations/${account.installation_id}`}
                                target="_blank"
                                rel="noopener noreferrer"
                              >
                                Configure
                              </a>
                            </Button>
                          </div>
                        ))
                      ) : (
                        <div className="flex flex-col gap-4 rounded-lg border bg-muted/30 p-4 sm:flex-row sm:items-center sm:justify-between">
                          <div className="flex items-center gap-4">
                            <div className="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
                              <FaGithub />
                            </div>
                            <div className="min-w-0">
                              <p className="text-sm font-semibold">GitHub app integration</p>
                              <p className="text-xs text-muted-foreground">
                                Deploy from any repository you have access to.
                              </p>
                            </div>
                          </div>
                          <Button size="sm" onClick={handleConnectGithub}>Connect GitHub</Button>
                        </div>
                      )}

                      {githubAccounts && githubAccounts.length > 0 && (
                        <div className="flex justify-start">
                          <Button variant="outline" size="sm" onClick={handleConnectGithub}>
                            <Plus data-icon="inline-start" />
                            Connect another account
                          </Button>
                        </div>
                      )}
                    </FieldGroup>
                  </FieldSet>
                </CardContent>
              </Card>
            </TabsContent>

            <TabsContent value="notifications" className="m-0">
              <Card>
                <CardHeader>
                  <CardTitle>Notifications</CardTitle>
                  <CardDescription>Choose what updates you want to receive via email.</CardDescription>
                </CardHeader>
                <CardContent>
                  <FieldSet>
                    <FieldLegend variant="label">Email notifications</FieldLegend>
                    <FieldGroup className="gap-6">
                      <Field orientation="horizontal" className="items-start rounded-lg border bg-muted/30 p-4">
                        <FieldContent>
                          <FieldLabel className="text-base">Deployment status</FieldLabel>
                          <FieldDescription>
                            Receive an email when your deployments finish or fail.
                          </FieldDescription>
                        </FieldContent>
                        <Switch checked={emailNotifications} onCheckedChange={setEmailNotifications} />
                      </Field>

                      <Field orientation="horizontal" className="items-start rounded-lg border bg-muted/30 p-4">
                        <FieldContent>
                          <FieldLabel className="text-base">Marketing emails</FieldLabel>
                          <FieldDescription>New features, tips and weekly summaries.</FieldDescription>
                        </FieldContent>
                        <Switch checked={marketingEmails} onCheckedChange={setMarketingNotifications} />
                      </Field>
                    </FieldGroup>
                  </FieldSet>
                </CardContent>
              </Card>
            </TabsContent>
          </Tabs>
        </div>
      </DashboardLayout>
    </AuthGuard>
  );
}
