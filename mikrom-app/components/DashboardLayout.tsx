"use client";

import React from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { 
  HiChartPie, 
  HiServer, 
  HiCog, 
  HiLogout, 
  HiCube
} from "react-icons/hi";
import { 
  Sidebar, 
  SidebarItems, 
  SidebarItemGroup, 
  SidebarItem, 
  Navbar, 
  NavbarBrand, 
  NavbarToggle, 
  NavbarCollapse,
  NavbarLink,
  Button, 
  DarkThemeToggle 
} from "flowbite-react";
import { logout } from "@/lib/auth";

export function DashboardLayout({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();

  return (
    <div className="min-h-screen bg-zinc-50 dark:bg-zinc-950">
      {/* Top Navbar */}
      <Navbar fluid rounded className="border-b border-zinc-200 dark:border-zinc-800 dark:bg-zinc-900 sticky top-0 z-40">
        <NavbarBrand as={Link} href="/">
          <HiCube className="mr-3 h-6 w-6 text-zinc-900 dark:text-white" />
          <span className="self-center whitespace-nowrap text-xl font-bold dark:text-white">
            Mikrom
          </span>
        </NavbarBrand>
        <div className="flex md:order-2 gap-2">
          <DarkThemeToggle />
          <Button color="gray" size="sm" onClick={() => logout()} className="hidden md:flex">
            <HiLogout className="w-4 h-4 mr-2" />
            Logout
          </Button>
          <NavbarToggle />
        </div>
        
        {/* Mobile menu items */}
        <NavbarCollapse>
          <NavbarLink as={Link} href="/" active={pathname === "/"}>
            Dashboard
          </NavbarLink>
          <NavbarLink as={Link} href="/vms" active={pathname.startsWith("/vms")}>
            Virtual Machines
          </NavbarLink>
          <NavbarLink as={Link} href="/settings" active={pathname === "/settings"}>
            Settings
          </NavbarLink>
          <NavbarLink href="#" onClick={() => logout()} className="md:hidden text-red-600">
            Logout
          </NavbarLink>
        </NavbarCollapse>
      </Navbar>

      <div className="flex">
        {/* Desktop Sidebar */}
        <Sidebar className="hidden md:block fixed left-0 top-16 h-[calc(100vh-64px)] w-64 border-r border-zinc-200 dark:border-zinc-800">
          <SidebarItems>
            <SidebarItemGroup>
              <SidebarItem 
                as={Link}
                href="/" 
                icon={HiChartPie}
                active={pathname === "/"}
              >
                Dashboard
              </SidebarItem>
              <SidebarItem 
                as={Link}
                href="/vms" 
                icon={HiServer}
                active={pathname.startsWith("/vms")}
              >
                Virtual Machines
              </SidebarItem>
              <SidebarItem 
                as={Link}
                href="/settings" 
                icon={HiCog}
                active={pathname === "/settings"}
              >
                Settings
              </SidebarItem>
            </SidebarItemGroup>
          </SidebarItems>
        </Sidebar>

        {/* Main Content */}
        <main className="flex-1 md:ml-64 p-6 min-h-[calc(100vh-64px)]">
          {children}
        </main>
      </div>
    </div>
  );
}
