"use client";

import { useState } from "react";
import Link from "next/link";
import { AuthGuard } from "@/components/AuthGuard";
import { logout } from "@/lib/auth";

interface App {
  id: string;
  name: string;
  status: "running" | "stopped" | "deploying";
  url: string;
  createdAt: string;
}

export default function DashboardPage() {
  const [apps] = useState<App[]>([
    {
      id: "1",
      name: "my-first-app",
      status: "running",
      url: "https://my-first-app.mikrom.cloud",
      createdAt: "2024-04-01",
    },
    {
      id: "2",
      name: "api-service",
      status: "stopped",
      url: "https://api-service.mikrom.cloud",
      createdAt: "2024-04-05",
    },
  ]);

  const handleLogout = () => {
    logout();
  };

  return (
    <AuthGuard>
      <div className="min-h-screen bg-zinc-50 dark:bg-zinc-950">
        <header className="bg-white dark:bg-zinc-900 border-b border-zinc-200 dark:border-zinc-800">
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
            <div className="flex justify-between items-center h-16">
              <div className="flex items-center gap-8">
                <Link
                  href="/dashboard"
                  className="text-xl font-bold text-zinc-900 dark:text-zinc-50"
                >
                  Mikrom
                </Link>
                <nav className="hidden md:flex gap-6">
                  <Link
                    href="/dashboard"
                    className="text-sm font-medium text-zinc-900 dark:text-zinc-100"
                  >
                    Dashboard
                  </Link>
                  <Link
                    href="/dashboard/apps"
                    className="text-sm font-medium text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100"
                  >
                    Apps
                  </Link>
                </nav>
              </div>
              <button
                onClick={handleLogout}
                className="text-sm text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100"
              >
                Logout
              </button>
            </div>
          </div>
        </header>

        <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
          <div className="mb-8">
            <h1 className="text-2xl font-bold text-zinc-900 dark:text-zinc-50">
              Dashboard
            </h1>
            <p className="text-zinc-600 dark:text-zinc-400 mt-1">
              Manage your applications on Mikrom Cloud Platform
            </p>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-6 mb-8">
            <div className="bg-white dark:bg-zinc-900 rounded-xl p-6 border border-zinc-200 dark:border-zinc-800">
              <div className="text-3xl font-bold text-zinc-900 dark:text-zinc-50">
                {apps.length}
              </div>
              <div className="text-sm text-zinc-600 dark:text-zinc-400 mt-1">
                Total Apps
              </div>
            </div>
            <div className="bg-white dark:bg-zinc-900 rounded-xl p-6 border border-zinc-200 dark:border-zinc-800">
              <div className="text-3xl font-bold text-green-600 dark:text-green-400">
                {apps.filter((a) => a.status === "running").length}
              </div>
              <div className="text-sm text-zinc-600 dark:text-zinc-400 mt-1">
                Running
              </div>
            </div>
            <div className="bg-white dark:bg-zinc-900 rounded-xl p-6 border border-zinc-200 dark:border-zinc-800">
              <div className="text-3xl font-bold text-zinc-400 dark:text-zinc-600">
                {apps.filter((a) => a.status === "stopped").length}
              </div>
              <div className="text-sm text-zinc-600 dark:text-zinc-400 mt-1">
                Stopped
              </div>
            </div>
          </div>

          <div className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800">
            <div className="px-6 py-4 border-b border-zinc-200 dark:border-zinc-800 flex justify-between items-center">
              <h2 className="text-lg font-semibold text-zinc-900 dark:text-zinc-50">
                Your Applications
              </h2>
              <button className="px-4 py-2 bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 text-sm font-medium rounded-lg hover:bg-zinc-800 dark:hover:bg-zinc-200 transition">
                Deploy New App
              </button>
            </div>
            <div className="divide-y divide-zinc-200 dark:divide-zinc-800">
              {apps.length === 0 ? (
                <div className="px-6 py-12 text-center">
                  <p className="text-zinc-600 dark:text-zinc-400">
                    No applications yet. Deploy your first app!
                  </p>
                </div>
              ) : (
                apps.map((app) => (
                  <div
                    key={app.id}
                    className="px-6 py-4 flex items-center justify-between"
                  >
                    <div className="flex items-center gap-4">
                      <div
                        className={`w-2 h-2 rounded-full ${
                          app.status === "running"
                            ? "bg-green-500"
                            : app.status === "deploying"
                            ? "bg-yellow-500"
                            : "bg-zinc-400"
                        }`}
                      />
                      <div>
                        <div className="font-medium text-zinc-900 dark:text-zinc-100">
                          {app.name}
                        </div>
                        <div className="text-sm text-zinc-500 dark:text-zinc-400">
                          {app.url}
                        </div>
                      </div>
                    </div>
                    <div className="flex items-center gap-4">
                      <span
                        className={`text-xs font-medium px-2 py-1 rounded-full ${
                          app.status === "running"
                            ? "bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400"
                            : app.status === "deploying"
                            ? "bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400"
                            : "bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400"
                        }`}
                      >
                        {app.status.charAt(0).toUpperCase() + app.status.slice(1)}
                      </span>
                      <button className="text-sm text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100">
                        Manage
                      </button>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
        </main>
      </div>
    </AuthGuard>
  );
}
