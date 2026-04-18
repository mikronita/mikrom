"use client";

import { useEffect, useState } from "react";
import { 
  Plus, 
  RefreshCw, 
  Server, 
  Search,
  ExternalLink,
  AlertCircle,
  Filter
} from "lucide-react";
import Link from "next/link";

import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { getToken } from "@/lib/auth";
import { listVms, VmInfo } from "@/lib/api";

import { Button } from "@/components/ui/Button";
import { Badge } from "@/components/ui/Badge";
import { Card, CardContent, CardHeader } from "@/components/ui/Card";
import { Input } from "@/components/ui/Input";
import { Skeleton } from "@/components/ui/Skeleton";
import { cn } from "@/lib/utils";

function getStatusVariant(status: string): "success" | "warning" | "danger" | "secondary" {
  const s = status.toLowerCase();
  if (s === "running") return "success";
  if (s === "scheduled" || s === "pending") return "warning";
  if (s === "failed" || s === "cancelled") return "danger";
  return "secondary";
}

export default function VmsPage() {
  const [vms, setVms] = useState<VmInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");

  const fetchVms = React.useCallback(async () => {
    const token = getToken();
    if (!token) return;
    setLoading(true);
    setError(null);
    const result = await listVms(token);
    if (result.error) {
      setError(result.error);
    } else {
      setVms(result.data ?? []);
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    const init = async () => {
      await fetchVms();
    };
    init();
  }, [fetchVms]);

  const filteredVms = vms.filter(vm => 
    vm.app_name.toLowerCase().includes(searchQuery.toLowerCase()) ||
    vm.image.toLowerCase().includes(searchQuery.toLowerCase()) ||
    vm.job_id.toLowerCase().includes(searchQuery.toLowerCase())
  );

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="p-8 space-y-8">
          <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
            <div>
              <h1 className="text-3xl font-bold text-zinc-900 dark:text-zinc-50 tracking-tight">
                Virtual Machines
              </h1>
              <p className="text-zinc-500 dark:text-zinc-400 mt-1">
                Manage and monitor all your running instances.
              </p>
            </div>
            <div className="flex items-center gap-3">
              <Button variant="outline" size="sm" onClick={fetchVms} disabled={loading}>
                <RefreshCw className={cn("w-4 h-4 mr-2", loading && "animate-spin")} />
                Refresh
              </Button>
              <Link href="/dashboard">
                <Button size="sm">
                  <Plus className="w-4 h-4 mr-2" />
                  New Instance
                </Button>
              </Link>
            </div>
          </div>

          <Card>
            <CardHeader className="border-b border-zinc-100 dark:border-zinc-800">
              <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
                <div className="relative w-full md:w-96">
                  <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-zinc-400" />
                  <Input 
                    placeholder="Filter by name, image or ID..." 
                    className="pl-9"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                  />
                </div>
                <div className="flex items-center gap-2">
                  <Button variant="outline" size="sm" className="h-9">
                    <Filter className="w-4 h-4 mr-2" />
                    Filters
                  </Button>
                </div>
              </div>
            </CardHeader>
            <CardContent className="p-0">
              {error && (
                <div className="p-4 flex items-center gap-3 text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/10 border-b border-red-100 dark:border-red-900/20">
                  <AlertCircle className="w-4 h-4" />
                  {error}
                </div>
              )}

              <div className="divide-y divide-zinc-100 dark:divide-zinc-800">
                {loading ? (
                  Array.from({ length: 5 }).map((_, i) => (
                    <div key={i} className="px-6 py-4 flex items-center justify-between">
                      <div className="flex items-center gap-4">
                        <Skeleton className="w-10 h-10 rounded-lg" />
                        <div className="space-y-2">
                          <Skeleton className="h-4 w-32" />
                          <Skeleton className="h-3 w-48" />
                        </div>
                      </div>
                      <Skeleton className="h-8 w-20 rounded-full" />
                    </div>
                  ))
                ) : filteredVms.length === 0 ? (
                  <div className="flex flex-col items-center justify-center py-24 text-center">
                    <div className="w-12 h-12 bg-zinc-100 dark:bg-zinc-800 rounded-full flex items-center justify-center mb-4 text-zinc-400">
                      <Server className="w-6 h-6" />
                    </div>
                    <p className="text-zinc-900 dark:text-zinc-100 font-medium">No instances found</p>
                    <p className="text-zinc-500 dark:text-zinc-400 text-sm mt-1 max-w-[250px]">
                      {searchQuery ? "Try adjusting your search terms." : "You don't have any virtual machines yet."}
                    </p>
                  </div>
                ) : (
                  filteredVms.map((vm) => (
                    <div
                      key={vm.job_id}
                      className="group px-6 py-4 flex items-center justify-between hover:bg-zinc-50 dark:hover:bg-zinc-800/30 transition-colors"
                    >
                      <div className="flex items-center gap-4">
                        <div className={cn(
                          "w-10 h-10 rounded-lg flex items-center justify-center border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-sm transition-transform group-hover:scale-110",
                          vm.status.toLowerCase() === "running" ? "text-green-600 dark:text-green-400" : "text-zinc-400"
                        )}>
                          <Server className="w-5 h-5" />
                        </div>
                        <div>
                          <div className="font-semibold text-zinc-900 dark:text-zinc-100 flex items-center gap-2">
                            {vm.app_name}
                            <Badge variant={getStatusVariant(vm.status)} className="capitalize px-1.5 py-0 h-4 text-[10px]">
                              {vm.status}
                            </Badge>
                          </div>
                          <div className="text-xs text-zinc-500 dark:text-zinc-400 font-mono mt-0.5">
                            {vm.image} • <span className="opacity-70">{vm.job_id.slice(0, 8)}...</span>
                          </div>
                        </div>
                      </div>
                      <div className="flex items-center gap-2">
                        <Link href={`/dashboard/vms/${vm.job_id}`}>
                          <Button variant="outline" size="sm">
                            View Details
                            <ExternalLink className="w-3 h-3 ml-2" />
                          </Button>
                        </Link>
                      </div>
                    </div>
                  ))
                )}
              </div>
            </CardContent>
          </Card>
        </div>
      </DashboardLayout>
    </AuthGuard>
  );
}
