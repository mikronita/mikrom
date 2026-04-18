"use client";

import React from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { 
  LayoutDashboard, 
  Server, 
  Settings, 
  LogOut, 
  Box,
  ChevronRight
} from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/Button";
import { logout } from "@/lib/auth";

interface SidebarItemProps {
  href: string;
  icon: React.ElementType;
  label: string;
  active?: boolean;
}

function SidebarItem({ href, icon: Icon, label, active }: SidebarItemProps) {
  return (
    <Link
      href={href}
      className={cn(
        "flex items-center gap-3 px-3 py-2 rounded-lg text-sm font-medium transition-colors",
        active 
          ? "bg-zinc-100 text-zinc-900 dark:bg-zinc-800 dark:text-zinc-50" 
          : "text-zinc-500 hover:text-zinc-900 hover:bg-zinc-50 dark:text-zinc-400 dark:hover:text-zinc-50 dark:hover:bg-zinc-800/50"
      )}
    >
      <Icon className="w-4 h-4" />
      {label}
      {active && <ChevronRight className="ml-auto w-4 h-4 opacity-50" />}
    </Link>
  );
}

export function DashboardLayout({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();

  return (
    <div className="flex min-h-screen bg-zinc-50 dark:bg-zinc-950">
      {/* Sidebar */}
      <aside className="w-64 border-r border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 hidden md:flex flex-col">
        <div className="p-6">
          <Link href="/dashboard" className="flex items-center gap-2 font-bold text-xl tracking-tight">
            <Box className="w-6 h-6 text-zinc-900 dark:text-zinc-50" />
            <span>Mikrom</span>
          </Link>
        </div>
        
        <nav className="flex-1 px-4 space-y-1">
          <SidebarItem 
            href="/dashboard" 
            icon={LayoutDashboard} 
            label="Dashboard" 
            active={pathname === "/dashboard"}
          />
          <SidebarItem 
            href="/dashboard/vms" 
            icon={Server} 
            label="Virtual Machines" 
            active={pathname.startsWith("/dashboard/vms")}
          />
          <SidebarItem 
            href="/dashboard/settings" 
            icon={Settings} 
            label="Settings" 
            active={pathname === "/dashboard/settings"}
          />
        </nav>

        <div className="p-4 border-t border-zinc-200 dark:border-zinc-800">
          <Button 
            variant="ghost" 
            className="w-full justify-start gap-3 text-zinc-500 hover:text-red-600 dark:hover:text-red-400"
            onClick={() => logout()}
          >
            <LogOut className="w-4 h-4" />
            Logout
          </Button>
        </div>
      </aside>

      {/* Main Content */}
      <div className="flex-1 flex flex-col">
        <header className="h-16 border-b border-zinc-200 dark:border-zinc-800 bg-white/50 dark:bg-zinc-900/50 backdrop-blur-sm md:hidden px-4 flex items-center justify-between">
          <Link href="/dashboard" className="font-bold text-lg tracking-tight flex items-center gap-2">
            <Box className="w-5 h-5" />
            Mikrom
          </Link>
          <Button variant="ghost" size="icon" onClick={() => logout()}>
            <LogOut className="w-4 h-4" />
          </Button>
        </header>

        <main className="flex-1 overflow-y-auto">
          {children}
        </main>
      </div>
    </div>
  );
}
