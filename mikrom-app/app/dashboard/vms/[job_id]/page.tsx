"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { AuthGuard } from "@/components/AuthGuard";
import { logout, getToken } from "@/lib/auth";
import { getVm, VmStatus } from "@/lib/api";

function statusBadge(status: string) {
  const s = status.toLowerCase();
  if (s === "running")
    return "bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400";
  if (s === "scheduled" || s === "pending")
    return "bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400";
  if (s === "failed" || s === "cancelled")
    return "bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-400";
  return "bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400";
}

function formatTimestamp(ts: number): string {
  if (!ts) return "—";
  return new Date(ts * 1000).toLocaleString();
}

function Row({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex flex-col sm:flex-row sm:items-center py-3 border-b border-zinc-200 dark:border-zinc-800 last:border-0">
      <dt className="text-sm font-medium text-zinc-500 dark:text-zinc-400 sm:w-40 shrink-0">
        {label}
      </dt>
      <dd className="mt-1 sm:mt-0 text-sm text-zinc-900 dark:text-zinc-100 font-mono break-all">
        {value}
      </dd>
    </div>
  );
}

export default function VmDetailPage() {
  const params = useParams<{ job_id: string }>();
  const jobId = params.job_id;

  const [vm, setVm] = useState<VmStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchVm = async () => {
    const token = getToken();
    if (!token) return;
    setLoading(true);
    setError(null);
    const result = await getVm(token, jobId);
    if (result.error) {
      setError(result.error);
    } else {
      setVm(result.data ?? null);
    }
    setLoading(false);
  };

  useEffect(() => {
    const token = getToken();
    if (!token) return;
    getVm(token, jobId).then((result) => {
      if (result.error) setError(result.error);
      else setVm(result.data ?? null);
      setLoading(false);
    });
  }, [jobId]);

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
                    className="text-sm font-medium text-zinc-500 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100"
                  >
                    Dashboard
                  </Link>
                  <span className="text-sm font-medium text-zinc-900 dark:text-zinc-100">
                    VM Detail
                  </span>
                </nav>
              </div>
              <button
                onClick={logout}
                className="text-sm text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100"
              >
                Logout
              </button>
            </div>
          </div>
        </header>

        <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
          <div className="mb-6 flex items-center justify-between">
            <div>
              <Link
                href="/dashboard"
                className="text-sm text-zinc-500 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100 mb-2 inline-block"
              >
                ← Back to Dashboard
              </Link>
              <h1 className="text-2xl font-bold text-zinc-900 dark:text-zinc-50">
                VM Detail
              </h1>
              <p className="text-zinc-500 dark:text-zinc-400 text-sm font-mono mt-1">
                {jobId}
              </p>
            </div>
            <button
              onClick={fetchVm}
              disabled={loading}
              className="px-3 py-2 text-sm text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100 disabled:opacity-50"
            >
              {loading ? "Loading…" : "Refresh"}
            </button>
          </div>

          {error && (
            <div className="mb-6 px-4 py-3 text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 border border-red-100 dark:border-red-900/30 rounded-lg">
              {error}
            </div>
          )}

          {loading && !vm ? (
            <div className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 px-6 py-12 text-center text-zinc-500 dark:text-zinc-400 text-sm">
              Loading…
            </div>
          ) : vm ? (
            <div className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 px-6 py-2">
              <dl>
                <Row
                  label="Status"
                  value={
                    <span
                      className={`inline-flex text-xs font-medium px-2 py-1 rounded-full font-sans ${statusBadge(vm.status)}`}
                    >
                      {vm.status}
                    </span>
                  }
                />
                <Row label="Job ID" value={vm.job_id} />
                <Row label="Host ID" value={vm.host_id || "—"} />
                <Row label="VM ID" value={vm.vm_id || "—"} />
                <Row label="Scheduled at" value={formatTimestamp(vm.scheduled_at)} />
                <Row label="Started at" value={formatTimestamp(vm.started_at)} />
                <Row label="Stopped at" value={formatTimestamp(vm.stopped_at)} />
                {vm.error_message && (
                  <Row
                    label="Error"
                    value={
                      <span className="text-red-600 dark:text-red-400 font-sans">
                        {vm.error_message}
                      </span>
                    }
                  />
                )}
              </dl>
            </div>
          ) : null}
        </main>
      </div>
    </AuthGuard>
  );
}
