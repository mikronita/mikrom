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
  HiCollection
} from "react-icons/hi";
import { 
  Sidebar, 
  SidebarItems, 
  SidebarItemGroup, 
  SidebarItem, 
  Navbar, 
  DarkThemeToggle,
  Avatar,
  Dropdown,
  DropdownHeader,
  DropdownItem,
  DropdownDivider,
  TextInput
} from "flowbite-react";
import { logout, getToken } from "@/lib/auth";
import { useQuery } from "@tanstack/react-query";
import { getUserProfile } from "@/lib/api";
import { useWatchVms } from "@/lib/hooks/use-vms";

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

  return (
    <div className="antialiased bg-gray-50 dark:bg-gray-900">
      {/* Navbar */}
      <Navbar fluid className="bg-white border-b border-gray-200 px-4 py-2.5 dark:bg-gray-800 dark:border-gray-700 fixed left-0 right-0 top-0 z-50">
        <div className="flex flex-wrap justify-between items-center w-full">
          <div className="flex justify-start items-center">
            <button
              onClick={() => setIsSidebarOpen(!isSidebarOpen)}
              className="p-2 mr-2 text-gray-600 rounded-lg cursor-pointer md:hidden hover:text-gray-900 hover:bg-gray-100 focus:bg-gray-100 dark:focus:bg-gray-700 focus:ring-2 focus:ring-gray-100 dark:focus:ring-gray-700 dark:text-gray-400 dark:hover:bg-gray-700 dark:hover:text-white"
            >
              {isSidebarOpen ? <HiX className="w-6 h-6" /> : <HiMenuAlt2 className="w-6 h-6" />}
            </button>
            <Link href="/" className="flex items-center justify-between mr-4">
              <HiCube className="mr-3 h-8 w-8 text-zinc-900 dark:text-white" />
              <span className="self-center text-2xl font-semibold whitespace-nowrap dark:text-white uppercase tracking-tighter">
                Mikrom
              </span>
            </Link>
            <form action="#" method="GET" className="hidden md:block md:pl-2">
              <label htmlFor="topbar-search" className="sr-only">Search</label>
              <TextInput
                id="topbar-search"
                icon={HiSearch}
                placeholder="Search resources..."
                className="w-72"
              />
            </form>
          </div>
          <div className="flex items-center lg:order-2 gap-2">
            <button
              type="button"
              className="p-2 mr-1 text-gray-500 rounded-lg hover:text-gray-900 hover:bg-gray-100 dark:text-gray-400 dark:hover:text-white dark:hover:bg-gray-700"
            >
              <span className="sr-only">View notifications</span>
              <HiBell className="w-6 h-6" />
            </button>
            <DarkThemeToggle />
            <Dropdown
              arrowIcon={false}
              inline
              label={
                <Avatar alt="User settings" img="" rounded placeholderInitials={initials} />
              }
            >
              <DropdownHeader>
                <span className="block text-sm font-bold">{fullName}</span>
                <span className="block truncate text-sm font-medium">{profile?.email}</span>
              </DropdownHeader>
              <DropdownItem as={Link} href="/settings">Settings</DropdownItem>
              <DropdownDivider />
              <DropdownItem onClick={() => logout()} className="text-red-600">Sign out</DropdownItem>
            </Dropdown>
          </div>
        </div>
      </Navbar>

      {/* Sidebar */}
      <aside
        className={`fixed top-0 left-0 z-40 w-64 h-screen pt-14 transition-transform ${
          isSidebarOpen ? "translate-x-0" : "-translate-x-full"
        } bg-white border-r border-gray-200 md:translate-x-0 dark:bg-gray-800 dark:border-gray-700`}
        aria-label="Sidenav"
      >
        <Sidebar className="h-full pt-5" theme={{ root: { inner: "bg-white dark:bg-gray-800 px-3 pb-4" } }}>
          <SidebarItems>
            <SidebarItemGroup>
              <SidebarItem 
                as={Link}
                href="/" 
                icon={HiChartPie}
                active={pathname === "/"}
                className={pathname === "/" ? "bg-gray-100 dark:bg-gray-700" : ""}
                onClick={() => setIsSidebarOpen(false)}
              >
                Dashboard
              </SidebarItem>
              <SidebarItem 
                as={Link}
                href="/apps" 
                icon={HiCollection}
                active={pathname.startsWith("/apps")}
                className={pathname.startsWith("/apps") ? "bg-gray-100 dark:bg-gray-700" : ""}
                onClick={() => setIsSidebarOpen(false)}
              >
                Applications
              </SidebarItem>
              <SidebarItem 
                as={Link}
                href="/settings" 
                icon={HiCog}
                active={pathname === "/settings"}
                className={pathname === "/settings" ? "bg-gray-100 dark:bg-gray-700" : ""}
                onClick={() => setIsSidebarOpen(false)}
              >
                Settings
              </SidebarItem>
            </SidebarItemGroup>
            <SidebarItemGroup>
              <SidebarItem 
                href="#"
                icon={HiLogout}
                onClick={() => logout()}
                className="text-red-600 hover:bg-red-50 dark:hover:bg-red-900/20"
              >
                Logout
              </SidebarItem>
            </SidebarItemGroup>
          </SidebarItems>
        </Sidebar>
      </aside>

      {/* Main Content */}
      <main className="p-4 md:ml-64 h-auto pt-20 min-h-screen">
        {children}
      </main>

      {/* Mobile Backdrop */}
      {isSidebarOpen && (
        <div 
          className="fixed inset-0 z-30 bg-gray-900/50 dark:bg-gray-900/80 md:hidden"
          onClick={() => setIsSidebarOpen(false)}
        />
      )}
    </div>
  );
}
