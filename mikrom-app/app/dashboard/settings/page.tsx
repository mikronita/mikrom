"use client";

import { 
  User, 
  Bell, 
  Key, 
  CreditCard,
  Cloud,
  CheckCircle2,
  Plus
} from "lucide-react";

import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { Button } from "@/components/ui/Button";
import { Badge } from "@/components/ui/Badge";
import { Card, CardContent, CardHeader, CardTitle, CardDescription, CardFooter } from "@/components/ui/Card";
import { Input } from "@/components/ui/Input";

function SettingsSection({ title, description, children, footer }: { 
  title: string; 
  description: string; 
  children: React.ReactNode;
  footer?: React.ReactNode;
}) {
  return (
    <Card className="overflow-hidden">
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {children}
      </CardContent>
      {footer && (
        <CardFooter className="bg-zinc-50 dark:bg-zinc-800/50 border-t border-zinc-100 dark:border-zinc-800 py-3">
          {footer}
        </CardFooter>
      )}
    </Card>
  );
}

export default function SettingsPage() {
  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="p-8 max-w-4xl mx-auto space-y-8">
          <div>
            <h1 className="text-3xl font-bold text-zinc-900 dark:text-zinc-50 tracking-tight">
              Settings
            </h1>
            <p className="text-zinc-500 dark:text-zinc-400 mt-1">
              Manage your account settings and preferences.
            </p>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-4 gap-8">
            <aside className="md:col-span-1 space-y-1">
              <Button variant="ghost" className="w-full justify-start gap-3 bg-zinc-100 dark:bg-zinc-800">
                <User className="w-4 h-4" />
                Profile
              </Button>
              <Button variant="ghost" className="w-full justify-start gap-3 text-zinc-500">
                <Key className="w-4 h-4" />
                Security
              </Button>
              <Button variant="ghost" className="w-full justify-start gap-3 text-zinc-500">
                <Bell className="w-4 h-4" />
                Notifications
              </Button>
              <Button variant="ghost" className="w-full justify-start gap-3 text-zinc-500">
                <CreditCard className="w-4 h-4" />
                Billing
              </Button>
            </aside>

            <div className="md:col-span-3 space-y-6">
              <SettingsSection 
                title="Profile Information" 
                description="Update your account details and email address."
                footer={
                  <div className="flex justify-between items-center w-full">
                    <p className="text-xs text-zinc-500">Last updated 2 days ago</p>
                    <Button size="sm">Save Changes</Button>
                  </div>
                }
              >
                <div className="grid grid-cols-2 gap-4">
                  <div className="space-y-2">
                    <label className="text-sm font-medium">First Name</label>
                    <Input placeholder="John" />
                  </div>
                  <div className="space-y-2">
                    <label className="text-sm font-medium">Last Name</label>
                    <Input placeholder="Doe" />
                  </div>
                </div>
                <div className="space-y-2">
                  <label className="text-sm font-medium">Email Address</label>
                  <Input type="email" placeholder="john@example.com" />
                </div>
              </SettingsSection>

              <SettingsSection 
                title="API Access" 
                description="Manage your API keys to access Mikrom from your own scripts."
              >
                <div className="p-4 border border-zinc-200 dark:border-zinc-800 rounded-xl bg-zinc-50 dark:bg-zinc-900/50 flex items-center justify-between">
                  <div className="flex items-center gap-3">
                    <div className="w-8 h-8 rounded-full bg-green-100 dark:bg-green-900/30 flex items-center justify-center">
                      <CheckCircle2 className="w-4 h-4 text-green-600" />
                    </div>
                    <div>
                      <p className="text-sm font-bold font-mono">mikrom_pk_live_****************</p>
                      <p className="text-xs text-zinc-500">Created on April 12, 2026</p>
                    </div>
                  </div>
                  <Button variant="outline" size="sm">Revoke</Button>
                </div>
                <Button variant="outline" className="w-full border-dashed">
                  <Plus className="w-4 h-4 mr-2" />
                  Generate New Key
                </Button>
              </SettingsSection>

              <SettingsSection 
                title="Region Preferences" 
                description="Default deployment region for your micro-VMs."
              >
                <div className="flex items-center gap-4 p-4 border border-zinc-200 dark:border-zinc-800 rounded-xl">
                  <div className="w-10 h-10 rounded-lg bg-zinc-100 dark:bg-zinc-800 flex items-center justify-center">
                    <Cloud className="w-5 h-5 text-zinc-500" />
                  </div>
                  <div className="flex-1">
                    <p className="text-sm font-bold">Europe West (Frankfurt)</p>
                    <p className="text-xs text-zinc-500">Latency: 12ms</p>
                  </div>
                  <Badge variant="success">Active</Badge>
                </div>
              </SettingsSection>

              <div className="pt-4">
                <Button variant="danger" className="w-full">
                  Delete Account
                </Button>
                <p className="text-center text-xs text-zinc-500 mt-2">
                  This action is permanent and cannot be undone.
                </p>
              </div>
            </div>
          </div>
        </div>
      </DashboardLayout>
    </AuthGuard>
  );
}
