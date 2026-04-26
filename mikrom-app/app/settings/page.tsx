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
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Switch } from "@/components/ui/switch";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { getUserProfile, updateUserProfile } from "@/lib/api";
import { getToken } from "@/lib/auth";
import { toast } from "sonner";
import { Loader2 } from "lucide-react";

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

  const initials = profile 
    ? `${profile.first_name?.[0] || ""}${profile.last_name?.[0] || ""}`.toUpperCase() || profile.email?.[0]?.toUpperCase() || "U"
    : "U";

  const updateMutation = useMutation({
    mutationFn: (data: { first_name: string; last_name: string }) => 
      updateUserProfile(token!, data).then(res => {
        if (res.error) throw new Error(res.error);
        return res.data;
      }),
    onSuccess: (data) => {
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
            <h1 className="text-3xl font-bold tracking-tight">
              Settings
            </h1>
            <p className="text-muted-foreground mt-1">
              Manage your personal information, security preferences and billing.
            </p>
          </div>

          <div className="bg-card rounded-2xl border shadow-sm overflow-hidden">
            <Tabs defaultValue="profile" className="w-full">
              <TabsList className="w-full justify-start border-b rounded-none h-auto p-0 bg-transparent">
                <TabsTrigger value="profile">
                  <HiUser className="mr-2 h-4 w-4" /> Profile
                </TabsTrigger>
                <TabsTrigger value="security">
                  <HiKey className="mr-2 h-4 w-4" /> Security
                </TabsTrigger>
                <TabsTrigger value="api">
                  <HiCloudDownload className="mr-2 h-4 w-4" /> API Access
                </TabsTrigger>
                <TabsTrigger value="billing">
                  <HiCreditCard className="mr-2 h-4 w-4" /> Billing
                </TabsTrigger>
                <TabsTrigger value="notifications">
                  <HiBell className="mr-2 h-4 w-4" /> Notifications
                </TabsTrigger>
              </TabsList>

              <TabsContent value="profile" className="p-0">
                {isLoading ? (
                  <div className="p-12 flex justify-center items-center">
                    <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
                  </div>
                ) : (
                  <div className="p-6 space-y-8">
                    <div className="flex flex-col sm:flex-row items-center gap-6 pb-6 border-b">
                      <Avatar className="h-20 w-20">
                        <AvatarFallback className="text-xl font-bold">{initials}</AvatarFallback>
                      </Avatar>
                      <div className="text-center sm:text-left space-y-2">
                        <h3 className="text-lg font-bold">Profile Picture</h3>
                        <p className="text-sm text-muted-foreground">JPG, GIF or PNG. Max size of 800K</p>
                        <div className="flex gap-2 justify-center sm:justify-start">
                          <Button size="sm">Upload New</Button>
                          <Button variant="destructive" size="sm">Delete</Button>
                        </div>
                      </div>
                    </div>

                    <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                      <div className="space-y-2">
                        <Label htmlFor="firstName">First Name</Label>
                        <Input 
                          id="firstName" 
                          placeholder="John" 
                          defaultValue={profile?.first_name || ""} 
                          onChange={(e) => setFirstName(e.target.value)} 
                        />
                      </div>
                      <div className="space-y-2">
                        <Label htmlFor="lastName">Last Name</Label>
                        <Input 
                          id="lastName" 
                          placeholder="Doe" 
                          defaultValue={profile?.last_name || ""} 
                          onChange={(e) => setLastName(e.target.value)} 
                        />
                      </div>
                      <div className="md:col-span-2 space-y-2">
                        <Label htmlFor="email">Email Address</Label>
                        <div className="relative">
                          <HiMail className="absolute left-3 top-3 h-4 w-4 text-muted-foreground" />
                          <Input 
                            id="email" 
                            type="email" 
                            placeholder="john@example.com" 
                            value={profile?.email || ""} 
                            disabled 
                            className="pl-9"
                          />
                        </div>
                        <p className="text-[0.8rem] text-muted-foreground">
                          Email cannot be changed yet.
                        </p>
                      </div>
                    </div>

                    <div className="flex justify-end">
                      <Button 
                        onClick={handleSave} 
                        disabled={updateMutation.isPending}
                      >
                        {updateMutation.isPending ? (
                          <>
                            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                            Saving...
                          </>
                        ) : (
                          "Save Changes"
                        )}
                      </Button>
                    </div>
                  </div>
                )}
              </TabsContent>

              <TabsContent value="security" className="p-6 space-y-8">
                <div>
                  <h3 className="text-lg font-bold mb-4">Change Password</h3>
                  <div className="grid grid-cols-1 gap-4 max-w-md">
                    <div className="space-y-2">
                      <Label htmlFor="currentPassword">Current Password</Label>
                      <Input id="currentPassword" type="password" />
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor="newPassword">New Password</Label>
                      <Input id="newPassword" type="password" />
                    </div>
                    <Button className="w-fit">Update Password</Button>
                  </div>
                </div>

                <div className="pt-8 border-t">
                  <div className="flex items-center justify-between mb-4">
                    <div>
                      <h3 className="text-lg font-bold">Two-Factor Authentication</h3>
                      <p className="text-sm text-muted-foreground">Add an extra layer of security to your account.</p>
                    </div>
                    <Badge variant="outline" className="text-yellow-600 border-yellow-200 dark:border-yellow-800">
                      <HiShieldCheck className="mr-1 h-3 w-3" /> Not Enabled
                    </Badge>
                  </div>
                  <Button size="sm">Configure 2FA</Button>
                </div>

                <div className="pt-8 border-t">
                  <h3 className="text-lg font-bold text-destructive mb-2">Danger Zone</h3>
                  <p className="text-sm text-muted-foreground mb-4">Once you delete your account, there is no going back. Please be certain.</p>
                  <Button variant="destructive" size="sm">
                    <HiTrash className="mr-2 h-4 w-4" />
                    Delete Account
                  </Button>
                </div>
              </TabsContent>

              <TabsContent value="api" className="p-6 space-y-6">
                <div className="flex items-center justify-between">
                  <div>
                    <h3 className="text-lg font-bold">Personal Access Tokens</h3>
                    <p className="text-sm text-muted-foreground">Use tokens to authenticate with the Mikrom CLI and API.</p>
                  </div>
                  <Button size="sm">
                    <HiPlus className="mr-2 h-4 w-4" />
                    Create New Token
                  </Button>
                </div>

                <div className="space-y-4">
                  <div className="p-4 rounded-xl border flex items-center justify-between bg-muted/30">
                    <div className="flex items-center gap-4">
                      <div className="h-10 w-10 rounded-lg bg-green-100 dark:bg-green-900/30 flex items-center justify-center">
                        <HiCheckCircle className="h-6 w-6 text-green-600" />
                      </div>
                      <div>
                        <p className="text-sm font-bold font-mono">mikrom_pk_live_****************</p>
                        <p className="text-xs text-muted-foreground">Last used 2 hours ago • Created April 12, 2026</p>
                      </div>
                    </div>
                    <Button variant="destructive" size="sm">Revoke</Button>
                  </div>
                </div>
              </TabsContent>

              <TabsContent value="billing" className="p-6 space-y-6">
                <Card className="bg-zinc-900 border-none text-white">
                  <CardHeader className="flex flex-row items-start justify-between">
                    <div>
                      <p className="text-zinc-400 text-xs uppercase font-bold tracking-widest mb-1">Current Plan</p>
                      <CardTitle className="text-2xl font-bold text-white">Pro Developer</CardTitle>
                      <CardDescription className="text-zinc-400 mt-1">$29 / month</CardDescription>
                    </div>
                    <Badge variant="secondary" className="bg-blue-500/20 text-blue-400 border-blue-500/30">Active</Badge>
                  </CardHeader>
                  <CardContent className="flex gap-2 pt-0">
                    <Button size="sm" className="bg-blue-600 hover:bg-blue-700 text-white border-none">Change Plan</Button>
                    <Button variant="outline" size="sm" className="text-zinc-300 border-zinc-700 hover:bg-zinc-800 hover:text-white">Cancel Subscription</Button>
                  </CardContent>
                </Card>

                <div className="pt-4">
                  <h3 className="text-lg font-bold mb-4">Payment Method</h3>
                  <div className="flex items-center gap-4 p-4 border rounded-xl">
                    <div className="w-12 h-8 bg-muted rounded flex items-center justify-center font-bold italic text-muted-foreground">VISA</div>
                    <div className="flex-1">
                      <p className="text-sm font-bold">Visa ending in 4242</p>
                      <p className="text-xs text-muted-foreground">Expires 12/28</p>
                    </div>
                    <Button variant="outline" size="sm">Edit</Button>
                  </div>
                </div>
              </TabsContent>

              <TabsContent value="notifications" className="p-6 space-y-6">
                <div>
                  <h3 className="text-lg font-bold mb-1">Email Notifications</h3>
                  <p className="text-sm text-muted-foreground mb-6">Choose what updates you want to receive via email.</p>
                  
                  <div className="space-y-6">
                    <div className="flex items-center justify-between">
                      <div className="space-y-0.5">
                        <Label className="text-base font-bold">Deployment Status</Label>
                        <p className="text-xs text-muted-foreground">Receive an email when your deployments finish or fail.</p>
                      </div>
                      <Switch checked={emailNotifications} onCheckedChange={setEmailNotifications} />
                    </div>
                    
                    <div className="flex items-center justify-between">
                      <div className="space-y-0.5">
                        <Label className="text-base font-bold">Marketing Emails</Label>
                        <p className="text-xs text-muted-foreground">New features, tips and weekly summaries.</p>
                      </div>
                      <Switch checked={marketingEmails} onCheckedChange={setMarketingNotifications} />
                    </div>
                  </div>
                </div>
              </TabsContent>
            </Tabs>
          </div>
        </div>
      </DashboardLayout>
    </AuthGuard>
  );
}
