"use client";

import React, { useState } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { 
  HiChartPie, 
  HiCog, 
  HiLogout, 
  HiCube,
  HiSearch,
  HiBell,
  HiMenuAlt2,
  HiX,
  HiCollection,
  HiMoon,
  HiSun
} from "react-icons/hi";
import { logout, getToken } from "@/lib/auth";
import { useQuery } from "@tanstack/react-query";
import { getUserProfile } from "@/lib/api";
import { useWatchVms } from "@/lib/hooks/use-vms";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { 
  DropdownMenu, 
  DropdownMenuContent, 
  DropdownMenuItem, 
  DropdownMenuLabel, 
  DropdownMenuSeparator, 
  DropdownMenuTrigger 
} from "@/components/ui/dropdown-menu";
import { 
  Breadcrumb, 
  BreadcrumbItem, 
  BreadcrumbLink, 
  BreadcrumbList, 
  BreadcrumbPage, 
  BreadcrumbSeparator 
} from "@/components/ui/breadcrumb";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { useTheme } from "next-themes";
import { cn } from "@/lib/utils";

function ThemeToggle() {
  const { theme, setTheme } = useTheme();

  return (
    <Button
      variant="ghost"
      size="icon"
      onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
      className="h-9 w-9"
    >
      <HiMoon className="h-[1.2rem] w-[1.2rem] rotate-0 scale-100 transition-all dark:-rotate-90 dark:scale-0" />
      <HiSun className="absolute h-[1.2rem] w-[1.2rem] rotate-90 scale-0 transition-all dark:rotate-0 dark:scale-100" />
      <span className="sr-only">Toggle theme</span>
    </Button>
  );
}

export function DashboardLayout({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const [isSidebarOpen, setIsSidebarOpen] = useState(false);
  const token = getToken();

  // Keep VMs synchronized globally via SSE
  useWatchVms();

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
    { name: "Dashboard", href: "/", icon: HiChartPie, active: pathname === "/" },
    { name: "Applications", href: "/apps", icon: HiCollection, active: pathname.startsWith("/apps") },
    { name: "Settings", href: "/settings", icon: HiCog, active: pathname === "/settings" },
  ];

  return (
    <div className="antialiased min-h-screen bg-background">
      {/* Navbar */}
      <nav className="fixed left-0 right-0 top-0 z-50 flex h-16 items-center border-b bg-background/95 backdrop-blur px-4">
        <div className="flex items-center justify-between w-full">
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setIsSidebarOpen(!isSidebarOpen)}
              className="md:hidden"
            >
              {isSidebarOpen ? <HiX className="h-6 w-6" /> : <HiMenuAlt2 className="h-6 w-6" />}
            </Button>
            <Link href="/" className="flex items-center gap-3 mr-4">
              <HiCube className="h-8 w-8" />
              <span className="hidden sm:inline-block text-xl font-bold uppercase tracking-tighter">
                Mikrom
              </span>
            </Link>
            <div className="hidden md:flex relative w-72">
              <HiSearch className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
              <Input
                type="search"
                placeholder="Search resources..."
                className="pl-9 h-9"
              />
            </div>
          </div>
          <div className="flex items-center gap-2">
            <Button variant="ghost" size="icon" className="h-9 w-9">
              <HiBell className="h-6 w-6" />
            </Button>
            <ThemeToggle />
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" className="relative h-9 w-9 rounded-full">
                  <Avatar className="h-9 w-9">
                    <AvatarFallback>{initials}</AvatarFallback>
                  </Avatar>
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent className="w-56" align="end" forceMount>
                <DropdownMenuLabel className="font-normal">
                  <div className="flex flex-col space-y-1">
                    <p className="text-sm font-medium leading-none">{fullName}</p>
                    <p className="text-xs leading-none text-muted-foreground">
                      {profile?.email}
                    </p>
                  </div>
                </DropdownMenuLabel>
                <DropdownMenuSeparator />
                <DropdownMenuItem asChild>
                  <Link href="/settings">Settings</Link>
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem onClick={() => logout()} className="text-destructive">
                  Sign out
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>
      </nav>

      {/* Sidebar */}
      <aside
        className={cn(
          "fixed top-0 left-0 z-40 w-64 h-screen pt-16 transition-transform border-r bg-background md:translate-x-0",
          isSidebarOpen ? "translate-x-0" : "-translate-x-full"
        )}
      >
        <div className="h-full px-3 py-4 flex flex-col justify-between">
          <div className="space-y-1">
            {navigation.map((item) => (
              <Link
                key={item.name}
                href={item.href}
                onClick={() => setIsSidebarOpen(false)}
                className={cn(
                  "flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors",
                  item.active 
                    ? "bg-secondary text-secondary-foreground" 
                    : "text-muted-foreground hover:bg-secondary/50 hover:text-foreground"
                )}
              >
                <item.icon className="h-5 w-5" />
                {item.name}
              </Link>
            ))}
          </div>
          <div className="pt-4 mt-4 border-t">
            <button
              onClick={() => logout()}
              className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium text-destructive transition-colors hover:bg-destructive/10"
            >
              <HiLogout className="h-5 w-5" />
              Logout
            </button>
          </div>
        </div>
      </aside>

      {/* Main Content */}
      <main className="p-4 md:ml-64 pt-20">
        <div className="max-w-7xl mx-auto space-y-6">
          <Breadcrumb>
            <BreadcrumbList>
              <BreadcrumbItem>
                <BreadcrumbLink asChild>
                  <Link href="/">Home</Link>
                </BreadcrumbLink>
              </BreadcrumbItem>
              {pathname !== "/" && <BreadcrumbSeparator />}
              {pathname
                .split("/")
                .filter((segment) => segment !== "")
                .map((segment, index, array) => {
                  const href = `/${array.slice(0, index + 1).join("/")}`;
                  const isLast = index === array.length - 1;
                  const decodedSegment = decodeURIComponent(segment);
                  const name = decodedSegment.charAt(0).toUpperCase() + decodedSegment.slice(1);

                  return (
                    <React.Fragment key={href}>
                      <BreadcrumbItem>
                        {isLast ? (
                          <BreadcrumbPage className="font-bold text-foreground">{name}</BreadcrumbPage>
                        ) : (
                          <BreadcrumbLink asChild>
                            <Link href={href} className="hover:text-foreground transition-colors">{name}</Link>
                          </BreadcrumbLink>
                        )}
                      </BreadcrumbItem>
                      {!isLast && <BreadcrumbSeparator />}
                    </React.Fragment>
                  );
                })}
            </BreadcrumbList>
          </Breadcrumb>
          {children}
        </div>
      </main>

      {/* Mobile Backdrop */}
      {isSidebarOpen && (
        <div 
          className="fixed inset-0 z-30 bg-background/80 backdrop-blur-sm md:hidden"
          onClick={() => setIsSidebarOpen(false)}
        />
      )}
    </div>
  );
}
