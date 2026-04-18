"use client";

import Link from "next/link";
import { 
  Box, 
  ArrowRight, 
  Zap, 
  Shield, 
  Cpu, 
  Cloud,
  CheckCircle2
} from "lucide-react";
import { isAuthenticated } from "@/lib/auth";
import { Button } from "@/components/ui/Button";

export default function Home() {
  const authenticated = typeof window !== "undefined" ? isAuthenticated() : false;

  return (
    <div className="flex flex-col min-h-screen bg-white dark:bg-zinc-950 selection:bg-zinc-900 selection:text-white dark:selection:bg-white dark:selection:text-black">
      {/* Navbar */}
      <nav className="border-b border-zinc-100 dark:border-zinc-900 bg-white/80 dark:bg-zinc-950/80 backdrop-blur-md sticky top-0 z-50">
        <div className="max-w-7xl mx-auto px-6 h-16 flex items-center justify-between">
          <div className="flex items-center gap-2 font-bold text-xl tracking-tight">
            <Box className="w-6 h-6" />
            <span>Mikrom</span>
          </div>
          <div className="flex items-center gap-4">
            {authenticated ? (
              <Link href="/dashboard">
                <Button size="sm">Go to Dashboard</Button>
              </Link>
            ) : (
              <>
                <Link href="/auth/login">
                  <Button variant="ghost" size="sm">Login</Button>
                </Link>
                <Link href="/auth/register">
                  <Button size="sm">Get Started</Button>
                </Link>
              </>
            )}
          </div>
        </div>
      </nav>

      <main className="flex-1">
        {/* Hero Section */}
        <section className="py-24 px-6 overflow-hidden relative">
          <div className="absolute top-0 left-1/2 -translate-x-1/2 w-full max-w-7xl h-full -z-10 opacity-10 dark:opacity-20 pointer-events-none">
            <div className="absolute top-0 left-1/4 w-96 h-96 bg-zinc-400 rounded-full blur-[128px]" />
            <div className="absolute bottom-0 right-1/4 w-96 h-96 bg-zinc-600 rounded-full blur-[128px]" />
          </div>

          <div className="max-w-4xl mx-auto text-center space-y-8">
            <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-zinc-100 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 text-xs font-medium animate-in fade-in slide-in-from-bottom-2 duration-500">
              <span className="relative flex h-2 w-2">
                <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-75"></span>
                <span className="relative inline-flex rounded-full h-2 w-2 bg-green-500"></span>
              </span>
              Now in Private Beta
            </div>
            
            <h1 className="text-5xl md:text-7xl font-extrabold tracking-tight text-zinc-900 dark:text-zinc-50 animate-in fade-in slide-in-from-bottom-4 duration-700">
              Compute at the <span className="text-zinc-500">Speed of Light.</span>
            </h1>
            
            <p className="text-xl text-zinc-600 dark:text-zinc-400 max-w-2xl mx-auto leading-relaxed animate-in fade-in slide-in-from-bottom-6 duration-1000">
              Deploy Firecracker micro-VMs in milliseconds. Mikrom provides the infrastructure 
              you need for high-performance, secure serverless workloads.
            </p>

            <div className="flex flex-col sm:flex-row items-center justify-center gap-4 pt-4 animate-in fade-in slide-in-from-bottom-8 duration-1000">
              <Link href={authenticated ? "/dashboard" : "/auth/register"}>
                <Button size="lg" className="rounded-full px-8">
                  {authenticated ? "Open Dashboard" : "Deploy your first app"}
                  <ArrowRight className="w-4 h-4 ml-2" />
                </Button>
              </Link>
              <Button variant="outline" size="lg" className="rounded-full px-8">
                Read Documentation
              </Button>
            </div>
          </div>
        </section>

        {/* Features Section */}
        <section className="py-24 bg-zinc-50 dark:bg-zinc-900/50 border-y border-zinc-100 dark:border-zinc-900">
          <div className="max-w-7xl mx-auto px-6">
            <div className="grid grid-cols-1 md:grid-cols-3 gap-12">
              <div className="space-y-4">
                <div className="w-12 h-12 rounded-2xl bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 flex items-center justify-center shadow-sm">
                  <Zap className="w-6 h-6 text-zinc-900 dark:text-zinc-50" />
                </div>
                <h3 className="text-xl font-bold">Lightning Fast</h3>
                <p className="text-zinc-600 dark:text-zinc-400 leading-relaxed">
                  Start VMs in less than 125ms. Our optimized Firecracker stack ensures 
                  your applications are ready exactly when you need them.
                </p>
              </div>

              <div className="space-y-4">
                <div className="w-12 h-12 rounded-2xl bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 flex items-center justify-center shadow-sm">
                  <Shield className="w-6 h-6 text-zinc-900 dark:text-zinc-50" />
                </div>
                <h3 className="text-xl font-bold">Secure by Default</h3>
                <p className="text-zinc-600 dark:text-zinc-400 leading-relaxed">
                  Hardware-level isolation for every workload. Multi-tenant security 
                  without the performance overhead of traditional virtualization.
                </p>
              </div>

              <div className="space-y-4">
                <div className="w-12 h-12 rounded-2xl bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 flex items-center justify-center shadow-sm">
                  <Cpu className="w-6 h-6 text-zinc-900 dark:text-zinc-50" />
                </div>
                <h3 className="text-xl font-bold">Resource Efficient</h3>
                <p className="text-zinc-600 dark:text-zinc-400 leading-relaxed">
                  Minimal memory footprint. Run thousands of micro-VMs on a single 
                  host with extreme density and optimal resource allocation.
                </p>
              </div>
            </div>
          </div>
        </section>

        {/* Trust Section */}
        <section className="py-24 px-6">
          <div className="max-w-7xl mx-auto bg-zinc-900 dark:bg-white rounded-3xl p-12 flex flex-col md:flex-row items-center justify-between gap-8 text-white dark:text-zinc-950 overflow-hidden relative">
            <div className="absolute top-0 right-0 w-64 h-64 bg-white/10 dark:bg-black/5 rounded-full blur-3xl -translate-y-1/2 translate-x-1/2" />
            
            <div className="space-y-4 max-w-xl">
              <h2 className="text-3xl md:text-4xl font-bold tracking-tight">Ready to scale your infrastructure?</h2>
              <p className="text-zinc-400 dark:text-zinc-500 text-lg">
                Join hundreds of developers building the next generation of 
                real-time applications on Mikrom.
              </p>
            </div>
            
            <div className="flex flex-col gap-3 min-w-[200px]">
              <div className="flex items-center gap-2 text-sm font-medium">
                <CheckCircle2 className="w-4 h-4 text-green-400 dark:text-green-600" />
                No credit card required
              </div>
              <div className="flex items-center gap-2 text-sm font-medium">
                <CheckCircle2 className="w-4 h-4 text-green-400 dark:text-green-600" />
                Free tier available
              </div>
              <Link href="/auth/register" className="mt-2">
                <Button variant="secondary" size="lg" className="w-full rounded-full dark:bg-zinc-900 dark:text-white">
                  Create Free Account
                </Button>
              </Link>
            </div>
          </div>
        </section>
      </main>

      {/* Footer */}
      <footer className="border-t border-zinc-100 dark:border-zinc-900 py-12 px-6">
        <div className="max-w-7xl mx-auto flex flex-col md:flex-row justify-between items-center gap-8">
          <div className="flex items-center gap-2 font-bold opacity-50">
            <Box className="w-5 h-5" />
            <span>Mikrom</span>
          </div>
          <div className="flex items-center gap-8 text-sm text-zinc-500">
            <a href="#" className="hover:text-zinc-900 dark:hover:text-zinc-50">Twitter</a>
            <a href="#" className="hover:text-zinc-900 dark:hover:text-zinc-50">GitHub</a>
            <a href="#" className="hover:text-zinc-900 dark:hover:text-zinc-50">Discord</a>
            <a href="#" className="hover:text-zinc-900 dark:hover:text-zinc-50">Terms</a>
          </div>
          <p className="text-sm text-zinc-400">
            © 2026 Mikrom Compute. All rights reserved.
          </p>
        </div>
      </footer>
    </div>
  );
}
