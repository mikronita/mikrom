"use client";

import { useState } from "react";
import {
  HiUser,
  HiBell,
  HiKey,
  HiCreditCard,
  HiCloudDownload,
  HiCheckCircle,
  HiPlus,
  HiTrash,
  HiShieldCheck,
  HiMail
} from "react-icons/hi";

import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { 
  Button, 
  Badge, 
  Card, 
  Label, 
  TextInput, 
  Tabs, 
  TabItem,
  Avatar,
  ToggleSwitch,
  Spinner,
  HelperText
} from "flowbite-react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { getUserProfile, updateUserProfile } from "@/lib/api";
import { getToken } from "@/lib/auth";
import { toast } from "sonner";

export default function SettingsPage() {
  const [emailNotifications, setEmailNotifications] = useState(true);
  const [marketingEmails, setMarketingNotifications] = useState(false);
  const [firstName, setFirstName] = useState("");
  const [lastName, setLastName] = useState("");
  
  const queryClient = useQueryClient();
  const token = getToken();

  const { data: profile, isLoading } = useQuery({
    queryKey: ["profile"],
    queryFn: () => getUserProfile(token!).then(res => {
      if (res.error) throw new Error(res.error);
      return res.data;
    }),
    enabled: !!token,
  });

  const updateMutation = useMutation({
    mutationFn: (data: { first_name: string; last_name: string }) => 
      updateUserProfile(token!, data).then(res => {
        if (res.error) throw new Error(res.error);
        return res.data;
      }),
    onSuccess: (data) => {
      // Actualizar el estado local con los datos devueltos para mantener la UI sincronizada
      if (data) {
        setFirstName(data.first_name || "");
        setLastName(data.last_name || "");
      }
      queryClient.invalidateQueries({ queryKey: ["profile"] });
      toast.success("Profile updated successfully");
    },
    onError: (error: Error) => {
      toast.error(error.message || "Failed to update profile");
    }
  });

  const handleSave = () => {
    updateMutation.mutate({ first_name: firstName, last_name: lastName });
  };

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="space-y-6">
          <div>
            <h1 className="text-3xl font-bold text-zinc-900 dark:text-zinc-50 tracking-tight">
              Settings
            </h1>
            <p className="text-zinc-500 dark:text-zinc-400 mt-1">
              Manage your personal information, security preferences and billing.
            </p>
          </div>

          <div className="bg-white dark:bg-zinc-900 rounded-2xl border border-zinc-200 dark:border-zinc-800 shadow-sm overflow-hidden">
            <Tabs aria-label="Settings tabs" variant="underline">
              {/* Profile Section */}
              <TabItem active title="Profile" icon={HiUser}>
                {isLoading ? (
                  <div className="p-12 flex justify-center items-center">
                    <Spinner size="xl" />
                  </div>
                ) : (
                  <div className="p-6 space-y-8" key={profile?.id}>
                    <div className="flex flex-col sm:flex-row items-center gap-6 pb-6 border-b border-zinc-100 dark:border-zinc-800">
                      <Avatar 
                        placeholderInitials={`${firstName?.[0] || profile?.first_name?.[0] || ""}${lastName?.[0] || profile?.last_name?.[0] || ""}` || "U"} 
                        size="xl" 
                        rounded 
                        className="ring-4 ring-zinc-50 dark:ring-zinc-800"
                      />
                      <div className="text-center sm:text-left space-y-2">
                        <h3 className="text-lg font-bold dark:text-white">Profile Picture</h3>
                        <p className="text-sm text-zinc-500">JPG, GIF or PNG. Max size of 800K</p>
                        <div className="flex gap-2 justify-center sm:justify-start">
                          <Button color="dark" size="xs">Upload New</Button>
                          <Button color="gray" size="xs" outline>Delete</Button>
                        </div>
                      </div>
                    </div>

                    <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                      <div>
                        <div className="mb-2 block">
                          <Label htmlFor="firstName">First Name</Label>
                        </div>
                        <TextInput 
                          id="firstName" 
                          placeholder="John" 
                          defaultValue={profile?.first_name || ""} 
                          onChange={(e) => setFirstName(e.target.value)} 
                        />
                      </div>
                      <div>
                        <div className="mb-2 block">
                          <Label htmlFor="lastName">Last Name</Label>
                        </div>
                        <TextInput 
                          id="lastName" 
                          placeholder="Doe" 
                          defaultValue={profile?.last_name || ""} 
                          onChange={(e) => setLastName(e.target.value)} 
                        />
                      </div>
                      <div className="md:col-span-2">
                        <div className="mb-2 block">
                          <Label htmlFor="email">Email Address</Label>
                        </div>
                        <TextInput 
                          id="email" 
                          type="email" 
                          icon={HiMail} 
                          placeholder="john@example.com" 
                          value={profile?.email || ""} 
                          disabled 
                        />
                        <HelperText className="mt-2 text-zinc-500">
                          Email cannot be changed yet.
                        </HelperText>
                      </div>
                    </div>

                    <div className="flex justify-end">
                      <Button 
                        color="primary" 
                        onClick={handleSave} 
                        disabled={updateMutation.isPending}
                      >
                        {updateMutation.isPending ? (
                          <>
                            <Spinner size="sm" className="mr-2" />
                            Saving...
                          </>
                        ) : (
                          "Save Changes"
                        )}
                      </Button>
                    </div>
                  </div>
                )}
              </TabItem>

              {/* Security Section */}
              <TabItem title="Security" icon={HiKey}>
                <div className="p-6 space-y-8">
                  <div>
                    <h3 className="text-lg font-bold dark:text-white mb-4">Change Password</h3>
                    <div className="grid grid-cols-1 gap-4 max-w-md">
                      <div>
                        <div className="mb-2 block">
                          <Label htmlFor="currentPassword">Current Password</Label>
                        </div>
                        <TextInput id="currentPassword" type="password" />
                      </div>
                      <div>
                        <div className="mb-2 block">
                          <Label htmlFor="newPassword">New Password</Label>
                        </div>
                        <TextInput id="newPassword" type="password" />
                      </div>
                      <Button color="dark" className="w-fit">Update Password</Button>
                    </div>
                  </div>

                  <div className="pt-8 border-t border-zinc-100 dark:border-zinc-800">
                    <div className="flex items-center justify-between mb-4">
                      <div>
                        <h3 className="text-lg font-bold dark:text-white">Two-Factor Authentication</h3>
                        <p className="text-sm text-zinc-500">Add an extra layer of security to your account.</p>
                      </div>
                      <Badge color="warning" icon={HiShieldCheck}>Not Enabled</Badge>
                    </div>
                    <Button color="gray" outline size="sm">Configure 2FA</Button>
                  </div>

                  <div className="pt-8 border-t border-zinc-100 dark:border-zinc-800">
                    <h3 className="text-lg font-bold text-red-600 dark:text-red-400 mb-2">Danger Zone</h3>
                    <p className="text-sm text-zinc-500 mb-4">Once you delete your account, there is no going back. Please be certain.</p>
                    <Button color="failure" size="sm">
                      <HiTrash className="mr-2 h-4 w-4" />
                      Delete Account
                    </Button>
                  </div>
                </div>
              </TabItem>

              {/* API Section */}
              <TabItem title="API Access" icon={HiCloudDownload}>
                <div className="p-6 space-y-6">
                  <div className="flex items-center justify-between">
                    <div>
                      <h3 className="text-lg font-bold dark:text-white">Personal Access Tokens</h3>
                      <p className="text-sm text-zinc-500">Use tokens to authenticate with the Mikrom CLI and API.</p>
                    </div>
                    <Button color="dark" size="sm">
                      <HiPlus className="mr-2 h-4 w-4" />
                      Create New Token
                    </Button>
                  </div>

                  <div className="space-y-4">
                    <div className="p-4 rounded-xl border border-zinc-200 dark:border-zinc-800 flex items-center justify-between bg-zinc-50 dark:bg-zinc-800/50">
                      <div className="flex items-center gap-4">
                        <div className="h-10 w-10 rounded-lg bg-green-100 dark:bg-green-900/30 flex items-center justify-center">
                          <HiCheckCircle className="h-6 w-6 text-green-600" />
                        </div>
                        <div>
                          <p className="text-sm font-bold font-mono dark:text-white">mikrom_pk_live_****************</p>
                          <p className="text-xs text-zinc-500">Last used 2 hours ago • Created April 12, 2026</p>
                        </div>
                      </div>
                      <Button color="gray" size="xs" outline>Revoke</Button>
                    </div>
                  </div>
                </div>
              </TabItem>

              {/* Billing Section */}
              <TabItem title="Billing" icon={HiCreditCard}>
                <div className="p-6 space-y-6">
                  <Card className="bg-zinc-900 border-none">
                    <div className="flex justify-between items-start text-white">
                      <div>
                        <p className="text-zinc-400 text-xs uppercase font-bold tracking-widest mb-1">Current Plan</p>
                        <h4 className="text-2xl font-bold">Pro Developer</h4>
                        <p className="text-zinc-400 text-sm mt-1">$29 / month</p>
                      </div>
                      <Badge color="info">Active</Badge>
                    </div>
                    <div className="mt-6 flex gap-2">
                      <Button color="light" size="sm">Change Plan</Button>
                      <Button color="failure" size="sm" outline className="text-white border-zinc-700 hover:bg-zinc-800">Cancel Subscription</Button>
                    </div>
                  </Card>

                  <div className="pt-4">
                    <h3 className="text-lg font-bold dark:text-white mb-4">Payment Method</h3>
                    <div className="flex items-center gap-4 p-4 border border-zinc-200 dark:border-zinc-800 rounded-xl">
                      <div className="w-12 h-8 bg-zinc-100 dark:bg-zinc-800 rounded flex items-center justify-center font-bold italic text-zinc-500">VISA</div>
                      <div className="flex-1">
                        <p className="text-sm font-bold dark:text-white">Visa ending in 4242</p>
                        <p className="text-xs text-zinc-500">Expires 12/28</p>
                      </div>
                      <Button color="gray" size="xs" outline>Edit</Button>
                    </div>
                  </div>
                </div>
              </TabItem>

              {/* Notifications Section */}
              <TabItem title="Notifications" icon={HiBell}>
                <div className="p-6 space-y-6">
                  <div>
                    <h3 className="text-lg font-bold dark:text-white mb-1">Email Notifications</h3>
                    <p className="text-sm text-zinc-500 mb-6">Choose what updates you want to receive via email.</p>
                    
                    <div className="space-y-6">
                      <div className="flex items-center justify-between">
                        <div>
                          <p className="text-sm font-bold dark:text-white">Deployment Status</p>
                          <p className="text-xs text-zinc-500">Receive an email when your deployments finish or fail.</p>
                        </div>
                        <ToggleSwitch checked={emailNotifications} onChange={setEmailNotifications} />
                      </div>
                      
                      <div className="flex items-center justify-between">
                        <div>
                          <p className="text-sm font-bold dark:text-white">Marketing Emails</p>
                          <p className="text-xs text-zinc-500">New features, tips and weekly summaries.</p>
                        </div>
                        <ToggleSwitch checked={marketingEmails} onChange={setMarketingNotifications} />
                      </div>
                    </div>
                  </div>
                </div>
              </TabItem>
            </Tabs>
          </div>
        </div>
      </DashboardLayout>
    </AuthGuard>
  );
}
