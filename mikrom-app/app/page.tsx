"use client";

import { isAuthenticated } from "@/lib/auth";

export default function Home() {
  const authenticated = typeof window !== "undefined" ? isAuthenticated() : false;

  const handleLogout = () => {
    if (typeof window !== "undefined") {
      localStorage.removeItem("mikrom_token");
      window.location.href = "/auth/login";
    }
  };

  return (
    <div className="flex flex-col flex-1 items-center justify-center bg-zinc-50 font-sans dark:bg-black">
      <main className="flex flex-1 w-full max-w-3xl flex-col items-center justify-between py-32 px-16 bg-white dark:bg-black sm:items-start">
        <div className="flex flex-col items-center gap-6 text-center sm:items-start sm:text-left">
          <h1 className="max-w-xs text-3xl font-semibold leading-10 tracking-tight text-black dark:text-zinc-50">
            Mikrom
          </h1>
          <p className="max-w-md text-lg leading-8 text-zinc-600 dark:text-zinc-400">
            Your micromobility management platform
          </p>
        </div>

        {authenticated ? (
          <div className="flex flex-col gap-4 mt-8">
            <p className="text-green-600 dark:text-green-400 font-medium">
              You are logged in
            </p>
            <button
              onClick={handleLogout}
              className="flex h-12 w-full items-center justify-center gap-2 rounded-full bg-zinc-900 dark:bg-zinc-100 px-5 text-white dark:text-zinc-900 transition-colors hover:bg-zinc-800 dark:hover:bg-zinc-200 md:w-[200px]"
            >
              Logout
            </button>
          </div>
        ) : (
          <div className="flex flex-col gap-4 text-base font-medium sm:flex-row mt-8">
            <a
              href="/auth/login"
              className="flex h-12 w-full items-center justify-center gap-2 rounded-full bg-zinc-900 dark:bg-zinc-100 px-5 text-white dark:text-zinc-900 transition-colors hover:bg-zinc-800 dark:hover:bg-zinc-200 md:w-[150px]"
            >
              Login
            </a>
            <a
              href="/auth/register"
              className="flex h-12 w-full items-center justify-center gap-2 rounded-full border border-solid border-black/[.08] px-5 transition-colors hover:bg-black/[.04] dark:border-white/[.145] dark:hover:bg-[#1a1a1a] md:w-[150px]"
            >
              Register
            </a>
          </div>
        )}
      </main>
    </div>
  );
}
