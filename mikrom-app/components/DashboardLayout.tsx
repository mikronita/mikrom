"use client";

import React from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { 
  HiSearch,
  HiBell,
  HiMoon,
  HiSun
} from "react-icons/hi";
import { useWatchVms } from "@/lib/hooks/use-vms";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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
import { AppSidebar } from "@/components/app-sidebar";
import { Separator } from "@/components/ui/separator";

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

  // Keep VMs synchronized globally via SSE
  useWatchVms();

  return (
    <SidebarProvider>
      <AppSidebar />
      <SidebarInset>
        <header className="flex h-16 shrink-0 items-center gap-4 border-b bg-background/50 backdrop-blur-md px-6 sticky top-0 z-10">
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
                            <BreadcrumbPage className="font-bold text-foreground truncate max-w-[100px] sm:max-w-none">{name}</BreadcrumbPage>
                          ) : (
                            <BreadcrumbLink asChild className="hidden sm:block">
                              <Link href={href} className="hover:text-foreground transition-colors">{name}</Link>
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
            <div className="hidden lg:flex relative w-64">
              <HiSearch className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
              <Input
                type="search"
                placeholder="Search..."
                className="pl-9 h-9 bg-muted/50 border-none"
              />
            </div>
            <div className="flex items-center gap-2">
              <Button variant="ghost" size="icon" className="h-9 w-9 rounded-full">
                <HiBell className="h-5 w-5" />
              </Button>
              <ThemeToggle />
            </div>
          </div>
        </header>
        <main className="flex-1 p-6">
          <div className="mx-auto w-full max-w-7xl">
            {children}
          </div>
        </main>
      </SidebarInset>
    </SidebarProvider>
  );
}
