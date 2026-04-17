"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { AuthGuard } from "@/components/AuthGuard";
import { logout, getToken } from "@/lib/auth";
import { listVms, deployApp, VmInfo, DeployRequest } from "@/lib/api";

function statusDot(status: string) {
  const s = status.toLowerCase();
  if (s === "running") return "bg-green-500";
  if (s === "scheduled" || s === "pending") return "bg-yellow-500";
  if (s === "failed" || s === "cancelled") return "bg-red-500";
  return "bg-zinc-400";
}

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

interface DeployForm {
  app_name: string;
  image: string;
  vcpus: string;
  memory_mib: string;
  disk_mib: string;
}

const EMPTY_FORM: DeployForm = {
  app_name: "",
  image: "",
  vcpus: "",
  memory_mib: "",
  disk_mib: "",
};

export default function DashboardPage() {
  const router = useRouter();
  const [vms, setVms] = useState<VmInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [showDeploy, setShowDeploy] = useState(false);
  const [form, setForm] = useState<DeployForm>(EMPTY_FORM);
  const [deploying, setDeploying] = useState(false);
  const [deployError, setDeployError] = useState<string | null>(null);

  const fetchVms = async () => {
    const token = getToken();
    if (!token) return;
    setLoading(true);
    setLoadError(null);
    const result = await listVms(token);
    if (result.error) {
      setLoadError(result.error);
    } else {
      setVms(result.data ?? []);
    }
    setLoading(false);
  };

  useEffect(() => {
    const token = getToken();
    if (!token) return;
    listVms(token).then((result) => {
      if (result.error) setLoadError(result.error);
      else setVms(result.data ?? []);
      setLoading(false);
    });
  }, []);

  const handleLogout = () => logout();

  const handleDeploySubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const token = getToken();
    if (!token) return;
    setDeploying(true);
    setDeployError(null);

    const payload: DeployRequest = {
      app_name: form.app_name,
      image: form.image,
    };
    if (form.vcpus) payload.vcpus = parseInt(form.vcpus, 10);
    if (form.memory_mib) payload.memory_mib = parseInt(form.memory_mib, 10);
    if (form.disk_mib) payload.disk_mib = parseInt(form.disk_mib, 10);

    const result = await deployApp(token, payload);
    setDeploying(false);

    if (result.error) {
      setDeployError(result.error);
      return;
    }

    setShowDeploy(false);
    setForm(EMPTY_FORM);
    await fetchVms();

    if (result.data?.job_id) {
      router.push(`/dashboard/vms/${result.data.job_id}`);
    }
  };

  const running = vms.filter((v) => v.status.toLowerCase() === "running").length;
  const scheduled = vms.filter(
    (v) =>
      v.status.toLowerCase() === "scheduled" ||
      v.status.toLowerCase() === "pending"
  ).length;

  return (
    <AuthGuard>
      <div className="min-h-screen bg-zinc-50 dark:bg-zinc-950">
        {/* Header */}
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

          {/* Stats */}
          <div className="grid grid-cols-1 md:grid-cols-3 gap-6 mb-8">
            <div className="bg-white dark:bg-zinc-900 rounded-xl p-6 border border-zinc-200 dark:border-zinc-800">
              <div className="text-3xl font-bold text-zinc-900 dark:text-zinc-50">
                {vms.length}
              </div>
              <div className="text-sm text-zinc-600 dark:text-zinc-400 mt-1">
                Total Apps
              </div>
            </div>
            <div className="bg-white dark:bg-zinc-900 rounded-xl p-6 border border-zinc-200 dark:border-zinc-800">
              <div className="text-3xl font-bold text-green-600 dark:text-green-400">
                {running}
              </div>
              <div className="text-sm text-zinc-600 dark:text-zinc-400 mt-1">
                Running
              </div>
            </div>
            <div className="bg-white dark:bg-zinc-900 rounded-xl p-6 border border-zinc-200 dark:border-zinc-800">
              <div className="text-3xl font-bold text-yellow-500 dark:text-yellow-400">
                {scheduled}
              </div>
              <div className="text-sm text-zinc-600 dark:text-zinc-400 mt-1">
                Deploying
              </div>
            </div>
          </div>

          {/* Apps list */}
          <div className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800">
            <div className="px-6 py-4 border-b border-zinc-200 dark:border-zinc-800 flex justify-between items-center">
              <h2 className="text-lg font-semibold text-zinc-900 dark:text-zinc-50">
                Your Applications
              </h2>
              <div className="flex gap-2">
                <button
                  onClick={fetchVms}
                  disabled={loading}
                  className="px-3 py-2 text-sm text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100 disabled:opacity-50"
                >
                  {loading ? "Loading…" : "Refresh"}
                </button>
                <button
                  onClick={() => {
                    setShowDeploy(true);
                    setDeployError(null);
                  }}
                  className="px-4 py-2 bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 text-sm font-medium rounded-lg hover:bg-zinc-800 dark:hover:bg-zinc-200 transition"
                >
                  Deploy New App
                </button>
              </div>
            </div>

            {loadError && (
              <div className="px-6 py-4 text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 border-b border-red-100 dark:border-red-900/30">
                {loadError}
              </div>
            )}

            <div className="divide-y divide-zinc-200 dark:divide-zinc-800">
              {loading && vms.length === 0 ? (
                <div className="px-6 py-12 text-center text-zinc-500 dark:text-zinc-400 text-sm">
                  Loading…
                </div>
              ) : vms.length === 0 ? (
                <div className="px-6 py-12 text-center">
                  <p className="text-zinc-600 dark:text-zinc-400">
                    No applications yet. Deploy your first app!
                  </p>
                </div>
              ) : (
                vms.map((vm) => (
                  <div
                    key={vm.job_id}
                    className="px-6 py-4 flex items-center justify-between"
                  >
                    <div className="flex items-center gap-4">
                      <div
                        className={`w-2 h-2 rounded-full ${statusDot(vm.status)}`}
                      />
                      <div>
                        <div className="font-medium text-zinc-900 dark:text-zinc-100">
                          {vm.app_name}
                        </div>
                        <div className="text-sm text-zinc-500 dark:text-zinc-400">
                          {vm.image}
                        </div>
                      </div>
                    </div>
                    <div className="flex items-center gap-4">
                      <span
                        className={`text-xs font-medium px-2 py-1 rounded-full ${statusBadge(vm.status)}`}
                      >
                        {vm.status}
                      </span>
                      <Link
                        href={`/dashboard/vms/${vm.job_id}`}
                        className="text-sm text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100"
                      >
                        Details
                      </Link>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
        </main>

        {/* Deploy modal */}
        {showDeploy && (
          <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
            <div className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 w-full max-w-md">
              <div className="px-6 py-4 border-b border-zinc-200 dark:border-zinc-800 flex justify-between items-center">
                <h3 className="text-lg font-semibold text-zinc-900 dark:text-zinc-50">
                  Deploy New App
                </h3>
                <button
                  onClick={() => setShowDeploy(false)}
                  className="text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100 text-xl leading-none"
                >
                  ×
                </button>
              </div>

              <form onSubmit={handleDeploySubmit} className="px-6 py-4 space-y-4">
                <div>
                  <label className="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">
                    App name <span className="text-red-500">*</span>
                  </label>
                  <input
                    type="text"
                    required
                    value={form.app_name}
                    onChange={(e) =>
                      setForm((f) => ({ ...f, app_name: e.target.value }))
                    }
                    placeholder="my-app"
                    className="w-full px-3 py-2 rounded-lg border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 text-sm focus:outline-none focus:ring-2 focus:ring-zinc-500"
                  />
                </div>

                <div>
                  <label className="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">
                    Image <span className="text-red-500">*</span>
                  </label>
                  <input
                    type="text"
                    required
                    value={form.image}
                    onChange={(e) =>
                      setForm((f) => ({ ...f, image: e.target.value }))
                    }
                    placeholder="nginx:latest or /opt/firecracker/rootfs.ext4"
                    className="w-full px-3 py-2 rounded-lg border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 text-sm focus:outline-none focus:ring-2 focus:ring-zinc-500"
                  />
                </div>

                <div className="grid grid-cols-3 gap-3">
                  <div>
                    <label className="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">
                      vCPUs
                    </label>
                    <input
                      type="number"
                      min="1"
                      value={form.vcpus}
                      onChange={(e) =>
                        setForm((f) => ({ ...f, vcpus: e.target.value }))
                      }
                      placeholder="1"
                      className="w-full px-3 py-2 rounded-lg border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 text-sm focus:outline-none focus:ring-2 focus:ring-zinc-500"
                    />
                  </div>
                  <div>
                    <label className="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">
                      RAM (MiB)
                    </label>
                    <input
                      type="number"
                      min="64"
                      value={form.memory_mib}
                      onChange={(e) =>
                        setForm((f) => ({ ...f, memory_mib: e.target.value }))
                      }
                      placeholder="256"
                      className="w-full px-3 py-2 rounded-lg border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 text-sm focus:outline-none focus:ring-2 focus:ring-zinc-500"
                    />
                  </div>
                  <div>
                    <label className="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">
                      Disk (MiB)
                    </label>
                    <input
                      type="number"
                      min="128"
                      value={form.disk_mib}
                      onChange={(e) =>
                        setForm((f) => ({ ...f, disk_mib: e.target.value }))
                      }
                      placeholder="1024"
                      className="w-full px-3 py-2 rounded-lg border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 text-sm focus:outline-none focus:ring-2 focus:ring-zinc-500"
                    />
                  </div>
                </div>

                {deployError && (
                  <p className="text-sm text-red-600 dark:text-red-400">
                    {deployError}
                  </p>
                )}

                <div className="flex justify-end gap-3 pt-2">
                  <button
                    type="button"
                    onClick={() => setShowDeploy(false)}
                    className="px-4 py-2 text-sm text-zinc-700 dark:text-zinc-300 hover:text-zinc-900 dark:hover:text-zinc-100"
                  >
                    Cancel
                  </button>
                  <button
                    type="submit"
                    disabled={deploying}
                    className="px-4 py-2 bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 text-sm font-medium rounded-lg hover:bg-zinc-800 dark:hover:bg-zinc-200 transition disabled:opacity-50"
                  >
                    {deploying ? "Deploying…" : "Deploy"}
                  </button>
                </div>
              </form>
            </div>
          </div>
        )}
      </div>
    </AuthGuard>
  );
}
