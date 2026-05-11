"use client";

import React from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { Bell, Moon, Search, Shield, Sun } from "lucide-react";
import { useWatchVms } from "@/lib/hooks/use-vms";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group";
import { 
  Breadcrumb, 
  BreadcrumbItem, 
  BreadcrumbLink, 
  BreadcrumbList, 
  BreadcrumbPage, 
  BreadcrumbSeparator 
} from "@/components/ui/breadcrumb";
import { useTheme } from "next-themes";
import {
  SidebarProvider,
  SidebarInset,
  SidebarTrigger
} from "@/components/ui/sidebar";
import { Separator } from "@/components/ui/separator";
import { AppSidebar } from "@/components/app-sidebar";

const footerColumns = [
  {
    title: "Platform",
    links: [
      { label: "Applications", href: "/apps" },
      { label: "Networking", href: "/networking" },
      { label: "Settings", href: "/settings" },
    ],
  },
  {
    title: "Resources",
    links: [
      { label: "Documentation", href: "#" },
      { label: "API Reference", href: "#" },
      { label: "Status", href: "#" },
    ],
  },
  {
    title: "Company",
    links: [
      { label: "About", href: "#" },
      { label: "Support", href: "#" },
      { label: "Contact", href: "#" },
    ],
  },
];

function ThemeToggle() {
  const { theme, setTheme } = useTheme();

  return (
    <Button
      variant="ghost"
      size="icon"
      onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
    >
      <Moon className="rotate-0 scale-100 transition-all dark:-rotate-90 dark:scale-0" />
      <Sun className="absolute rotate-90 scale-0 transition-all dark:rotate-0 dark:scale-100" />
      <span className="sr-only">Toggle theme</span>
    </Button>
  );
}

export function DashboardLayout({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();

  // Keep VMs synchronized globally via SSE
  useWatchVms();

  return (
    <SidebarProvider>
      <AppSidebar />
      <SidebarInset>
        <header className="sticky top-0 z-10 flex h-16 shrink-0 items-center gap-4 border-b bg-background/95 px-4 backdrop-blur supports-[backdrop-filter]:bg-background/80 md:px-6">
          <SidebarTrigger />
          <div className="flex-1 overflow-hidden">
            <Breadcrumb>
              <BreadcrumbList className="flex-nowrap">
                <BreadcrumbItem className="hidden md:block">
                  <BreadcrumbLink asChild>
                    <Link href="/">Home</Link>
                  </BreadcrumbLink>
                </BreadcrumbItem>
                {pathname !== "/" && <BreadcrumbSeparator className="hidden md:block" />}
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
                            <BreadcrumbPage className="max-w-[140px] truncate font-medium text-foreground sm:max-w-none">{name}</BreadcrumbPage>
                          ) : (
                            <BreadcrumbLink asChild className="hidden sm:block">
                              <Link href={href}>{name}</Link>
                            </BreadcrumbLink>
                          )}
                        </BreadcrumbItem>
                        {!isLast && <BreadcrumbSeparator className="hidden sm:block" />}
                      </React.Fragment>
                    );
                  })}
              </BreadcrumbList>
            </Breadcrumb>
          </div>
          <div className="flex items-center gap-4">
            <div className="hidden w-64 lg:block">
              <InputGroup>
                <InputGroupAddon>
                  <Search />
                </InputGroupAddon>
                <InputGroupInput type="search" placeholder="Search..." />
              </InputGroup>
            </div>
            <div className="flex items-center gap-2">
              <Button variant="ghost" size="icon">
                <Bell />
                <span className="sr-only">Notifications</span>
              </Button>
              <ThemeToggle />
            </div>
          </div>
        </header>
        <main className="flex-1 p-4 md:p-6">
          <div className="mx-auto w-full max-w-7xl">
            {children}
          </div>
        </main>
        <footer className="px-4 pb-6 md:px-6">
          <div className="mx-auto flex w-full max-w-7xl flex-col gap-6 rounded-lg border bg-card p-6 text-card-foreground shadow-sm">
            <div className="grid gap-8 lg:grid-cols-[1.2fr_2fr]">
              <div className="flex max-w-sm flex-col gap-4">
                <div className="flex items-center gap-3">
                  <div className="flex size-9 items-center justify-center rounded-md bg-primary text-primary-foreground">
                    <Shield />
                  </div>
                  <div className="flex flex-col">
                    <span className="text-sm font-semibold leading-none">Mikrom</span>
                    <span className="mt-1 text-xs text-muted-foreground">Cloud Platform</span>
                  </div>
                </div>
                <p className="text-sm leading-6 text-muted-foreground">
                  Deploy, operate and observe microVM-backed applications from one focused control plane.
                </p>
              </div>
              <div className="grid gap-8 sm:grid-cols-3">
                {footerColumns.map((column) => (
                  <div key={column.title} className="flex flex-col gap-3">
                    <h2 className="text-sm font-medium">{column.title}</h2>
                    <nav aria-label={column.title} className="flex flex-col gap-2">
                      {column.links.map((link) => (
                        <Link
                          key={link.label}
                          href={link.href}
                          className="text-sm text-muted-foreground transition-colors hover:text-foreground"
                        >
                          {link.label}
                        </Link>
                      ))}
                    </nav>
                  </div>
                ))}
              </div>
            </div>
            <Separator />
            <div className="flex flex-col gap-3 text-sm text-muted-foreground sm:flex-row sm:items-center sm:justify-between">
              <p>© {new Date().getFullYear()} Mikrom. All rights reserved.</p>
              <div className="flex flex-wrap gap-4">
                <Link href="#" className="transition-colors hover:text-foreground">Privacy</Link>
                <Link href="#" className="transition-colors hover:text-foreground">Terms</Link>
                <Link href="#" className="transition-colors hover:text-foreground">Security</Link>
              </div>
            </div>
          </div>
        </footer>
      </SidebarInset>
    </SidebarProvider>
  );
}
