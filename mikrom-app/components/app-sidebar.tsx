"use client";

import * as React from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { Boxes, ChevronsUpDown, LayoutDashboard, LogOut, Network, Settings, Shield } from "lucide-react";
import { logout, getToken } from "@/lib/auth";
import { useQuery } from "@tanstack/react-query";
import { getUserProfile } from "@/lib/api";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarGroupContent,
} from "@/components/ui/sidebar";

export function AppSidebar() {
  const pathname = usePathname();
  const token = getToken();

  const { data: profile } = useQuery({
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

  const fullName = profile?.first_name && profile?.last_name 
    ? `${profile.first_name} ${profile.last_name}`
    : profile?.email.split("@")[0] || "User";

  const navigation = [
    { name: "Dashboard", href: "/", icon: LayoutDashboard, active: pathname === "/" },
    { name: "Applications", href: "/apps", icon: Boxes, active: pathname.startsWith("/apps") },
    { name: "Networking", href: "/networking", icon: Network, active: pathname.startsWith("/networking") },
    { name: "Settings", href: "/settings", icon: Settings, active: pathname === "/settings" },
  ];

  return (
    <Sidebar>
      <SidebarHeader className="flex h-16 items-center border-b p-0">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton size="lg" asChild className="px-4 md:h-14">
              <Link href="/" className="flex items-center gap-3">
                <div className="flex size-8 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground">
                  <Shield />
                </div>
                <div className="flex flex-col overflow-hidden">
                  <span className="whitespace-nowrap text-sm font-semibold leading-none">Mikrom</span>
                  <span className="mt-1 text-xs text-muted-foreground">Cloud Platform</span>
                </div>
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>
            Management
          </SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {navigation.map((item) => (
                <SidebarMenuItem key={item.name}>
                  <SidebarMenuButton
                    asChild
                    isActive={item.active}
                    tooltip={item.name}
                  >
                    <Link href={item.href} className="flex items-center">
                      <item.icon />
                      <span>{item.name}</span>
                    </Link>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter className="border-t">
        <SidebarMenu>
          <SidebarMenuItem>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <SidebarMenuButton
                  size="lg"
                  className="data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground"
                >
                  <Avatar className="size-8 shrink-0 rounded-lg">
                    <AvatarFallback className="rounded-lg text-xs font-medium">{initials}</AvatarFallback>
                  </Avatar>
                  <div className="grid flex-1 text-left text-sm leading-tight">
                    <span className="truncate font-medium">{fullName}</span>
                    <span className="truncate text-xs text-muted-foreground">{profile?.email}</span>
                  </div>
                  <ChevronsUpDown className="ml-auto" />
                </SidebarMenuButton>
              </DropdownMenuTrigger>
              <DropdownMenuContent
                className="w-(--radix-dropdown-menu-trigger-width) min-w-56 rounded-lg"
                side="bottom"
                align="end"
                sideOffset={4}
              >
                <DropdownMenuLabel className="p-0 font-normal">
                  <div className="flex items-center gap-2 px-1 py-1.5 text-left text-sm">
                    <Avatar className="h-8 w-8 rounded-lg">
                      <AvatarFallback className="rounded-lg">{initials}</AvatarFallback>
                    </Avatar>
                    <div className="grid flex-1 text-left text-sm leading-tight">
                      <span className="truncate font-semibold">{fullName}</span>
                      <span className="truncate text-xs">{profile?.email}</span>
                    </div>
                  </div>
                </DropdownMenuLabel>
                <DropdownMenuSeparator />
                <DropdownMenuItem asChild>
                  <Link href="/settings">Settings</Link>
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem onClick={() => logout()} className="text-destructive">
                  <LogOut data-icon="inline-start" />
                  Sign out
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
}
