import { expect, type Page } from "@playwright/test";
import {
  appDeployments,
  apps,
  authToken,
  databases,
  deployments,
  mesh,
  profile,
  projects,
  securityRules,
  volumeSnapshots,
  volumes,
} from "./fixtures";

const apiBase = "/api/v1";

function jsonResponse(body: unknown, status = 200) {
  return {
    status,
    contentType: "application/json",
    body: JSON.stringify(body),
  };
}

function sseResponse(body = "") {
  return {
    status: 200,
    contentType: "text/event-stream",
    body,
  };
}

export async function seedAuth(page: Page, token: string = authToken) {
  await page.addInitScript((value) => {
    localStorage.setItem("mikrom_token", value);
  }, token);
}

export async function installBrowserShims(page: Page) {
  await page.addInitScript(() => {
    class MockEventSource {
      url: string;
      onmessage: ((event: MessageEvent) => void) | null = null;
      onerror: ((event: Event) => void) | null = null;

      constructor(url: string) {
        this.url = url;
      }

      close() {}
    }

    // @ts-expect-error - Playwright injects into the browser runtime.
    window.EventSource = MockEventSource;

    if (!window.matchMedia) {
      window.matchMedia = ((query: string) => ({
        matches: false,
        media: query,
        onchange: null,
        addListener() {},
        removeListener() {},
        addEventListener() {},
        removeEventListener() {},
        dispatchEvent() {
          return false;
        },
      })) as typeof window.matchMedia;
    }
  });
}

type MockControlPlaneApiOptions = {
  githubWebhookSecret?: string;
};

export async function mockControlPlaneApi(
  page: Page,
  options: MockControlPlaneApiOptions = {},
) {
  let projectsState = [...projects];
  let databasesState = [...databases];
  const databaseSnapshotsState: Record<string, Array<{ id: string; name: string; created_at: number; size_bytes: number; vm_status: string }>> = {
    "db-1": [],
  };
  let securityRulesState = [...securityRules];
  let appsState = [...apps];
  let volumesState = [...volumes];

  await page.route("**/api/v1/**", async (route) => {
    const request = route.request();
    const { pathname } = new URL(request.url());
    const method = request.method();

    if (pathname === `${apiBase}/auth/login` && method === "POST") {
      await route.fulfill(jsonResponse({ token: authToken }));
      return;
    }

    if (pathname === `${apiBase}/auth/register` && method === "POST") {
      await route.fulfill(jsonResponse({ message: "Account created", user_id: "user-1" }));
      return;
    }

    if (pathname === `${apiBase}/auth/me` && method === "GET") {
      await route.fulfill(jsonResponse(profile));
      return;
    }

    if (pathname === `${apiBase}/projects` && method === "GET") {
      await route.fulfill(jsonResponse(projectsState));
      return;
    }

    if (pathname === `${apiBase}/projects` && method === "POST") {
      const payload = request.postDataJSON() as { name?: string } | undefined;
      const nextIndex = projectsState.length + 1;
      const tenant_id = `proj${String(nextIndex).padStart(2, "0")}`;
      const createdProject = {
        id: `project-${nextIndex}`,
        tenant_id,
        name: payload?.name ?? `Project ${nextIndex}`,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
      };
      projectsState = [createdProject, ...projectsState];
      await route.fulfill(jsonResponse(createdProject, 201));
      return;
    }

    if (pathname.startsWith(`${apiBase}/projects/`)) {
      const tenantSlug = decodeURIComponent(pathname.slice(`${apiBase}/projects/`.length));

      if (method === "GET") {
        const project = projectsState.find((entry) => entry.tenant_id === tenantSlug);
        if (!project) {
          await route.fulfill(jsonResponse({ error: "Tenant not found", status: 404 }, 404));
          return;
        }

        await route.fulfill(jsonResponse(project));
        return;
      }

      if (method === "PATCH") {
        const payload = request.postDataJSON() as { name?: string } | undefined;
        const projectIndex = projectsState.findIndex((entry) => entry.tenant_id === tenantSlug);
        if (projectIndex === -1) {
          await route.fulfill(jsonResponse({ error: "Tenant not found", status: 404 }, 404));
          return;
        }

        const updatedProject = {
          ...projectsState[projectIndex],
          name: payload?.name ?? projectsState[projectIndex].name,
          updated_at: new Date().toISOString(),
        };
        projectsState[projectIndex] = updatedProject;
        await route.fulfill(jsonResponse(updatedProject));
        return;
      }

      if (method === "DELETE") {
        const project = projectsState.find((entry) => entry.tenant_id === tenantSlug);
        if (!project) {
          await route.fulfill(jsonResponse({ error: "Tenant not found", status: 404 }, 404));
          return;
        }

        if (tenantSlug === "acme" && (appsState.length > 0 || databasesState.length > 0 || volumesState.length > 0)) {
          await route.fulfill(
            jsonResponse(
              {
                error: "This project still has apps, databases or volumes. Remove them first.",
                status: 409,
              },
              409
            )
          );
          return;
        }

        projectsState = projectsState.filter((entry) => entry.tenant_id !== tenantSlug);
        await route.fulfill({ status: 204 });
        return;
      }

      return;
    }

    if (pathname === `${apiBase}/apps` && method === "GET") {
      await route.fulfill(jsonResponse(appsState));
      return;
    }

    if (pathname === `${apiBase}/databases` && method === "GET") {
      await route.fulfill(jsonResponse(databasesState));
      return;
    }

    if (pathname === `${apiBase}/databases` && method === "POST") {
      const payload = request.postDataJSON() as
        | {
            name?: string;
            engine?: string;
            postgres_version?: number;
            vcpus?: number;
            memory_mib?: number;
            disk_mib?: number;
          }
        | undefined;

      const nextIndex = databasesState.length + 1;
      const createdDatabase = {
        id: `db-${nextIndex}`,
        name: payload?.name ?? `database-${nextIndex}`,
        engine: payload?.engine ?? "neon",
        postgres_version: payload?.postgres_version ?? 16,
        status: "pending",
        vcpus: payload?.vcpus ?? 1,
        memory_mib: payload?.memory_mib ?? 512,
        disk_mib: payload?.disk_mib ?? 10240,
        active_deployment_id: null,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
      };
      databasesState = [createdDatabase, ...databasesState];
      databaseSnapshotsState[createdDatabase.id] = [];
      await route.fulfill(jsonResponse(createdDatabase, 201));
      return;
    }

    if (pathname.startsWith(`${apiBase}/databases/`)) {
      const relativePath = pathname.slice(`${apiBase}/databases/`.length);
      const databaseId = decodeURIComponent(relativePath.replace(/\/(connection|branches|backups.*)$/, ""));
      const database = databasesState.find((entry) => entry.id === databaseId);
      const hasActiveDeployment = Boolean(database?.active_deployment_id);

      if (pathname.endsWith("/connection") && method === "GET") {
        if (!database) {
          await route.fulfill(jsonResponse({ error: "Database not found", status: 404 }, 404));
          return;
        }

        if (!hasActiveDeployment) {
          await route.fulfill(
            jsonResponse({ error: "Database has no active deployment yet", status: 409 }, 409)
          );
          return;
        }

        await route.fulfill(
          jsonResponse({
            database_id: database.id,
            database_name: database.name,
            database_user: "cloud_admin",
            database_host: "127.0.0.1",
            database_port: 5432,
            ssh_host: "fd00:1234::99",
            ssh_user: "mikrom",
            ssh_port: 22,
            ssh_tunnel_command: "ssh -N -L 5432:127.0.0.1:5432 mikrom@[fd00:1234::99]",
            psql_command: `psql "host=127.0.0.1 port=5432 user=cloud_admin dbname=${database.name}"`,
          }),
        );
        return;
      }

      if (pathname.endsWith("/backups") && method === "GET") {
        if (!database) {
          await route.fulfill(jsonResponse({ error: "Database not found", status: 404 }, 404));
          return;
        }

        if (!hasActiveDeployment) {
          await route.fulfill(
            jsonResponse({ error: "Database has no active deployment yet", status: 409 }, 409)
          );
          return;
        }

        await route.fulfill(
          jsonResponse({
            database_id: database.id,
            database_name: database.name,
            backup_strategy: "continuous",
            recovery_mode: "point-in-time recovery available",
            retention_valid: true,
            neon_tenant_id: "tenant-1",
            neon_timeline_id: "timeline-1",
            tenant_gen: 1,
            status: database.status,
            created_at: database.created_at,
            updated_at: database.updated_at,
          })
        );
        return;
      }

      if (relativePath.endsWith("/backups/snapshots") && method === "GET") {
        if (!database) {
          await route.fulfill(jsonResponse({ error: "Database not found", status: 404 }, 404));
          return;
        }

        if (!hasActiveDeployment) {
          await route.fulfill(
            jsonResponse({ error: "Database has no active deployment yet", status: 409 }, 409)
          );
          return;
        }

        const snapshots = databaseSnapshotsState[databaseId] ?? [];
        await route.fulfill(
          jsonResponse({
            success: true,
            message: "Snapshot history",
            snapshots,
          })
        );
        return;
      }

      if (relativePath.endsWith("/backups/snapshots") && method === "POST") {
        if (!database) {
          await route.fulfill(jsonResponse({ error: "Database not found", status: 404 }, 404));
          return;
        }

        if (!hasActiveDeployment) {
          await route.fulfill(
            jsonResponse({ error: "Database has no active deployment yet", status: 409 }, 409)
          );
          return;
        }

        const payload = request.postDataJSON() as { name?: string } | undefined;
        const snapshot = {
          id: `snap-${Date.now()}`,
          name: payload?.name ?? "manual-snapshot",
          created_at: Date.now(),
          size_bytes: 2147483648,
          vm_status: "RUNNING",
        };
        databaseSnapshotsState[databaseId] = [snapshot, ...(databaseSnapshotsState[databaseId] ?? [])];
        await route.fulfill(
          jsonResponse({
            success: true,
            message: "Snapshot created",
          })
        );
        return;
      }

      if (relativePath.endsWith("/backups/restore") && method === "POST") {
        if (!database) {
          await route.fulfill(jsonResponse({ error: "Database not found", status: 404 }, 404));
          return;
        }

        if (!hasActiveDeployment) {
          await route.fulfill(
            jsonResponse({ error: "Database has no active deployment yet", status: 409 }, 409)
          );
          return;
        }

        await route.fulfill(
          jsonResponse({
            success: true,
            message: "Snapshot restored",
          })
        );
        return;
      }

      if (relativePath.includes("/backups/snapshots/") && method === "DELETE") {
        if (!database) {
          await route.fulfill(jsonResponse({ error: "Database not found", status: 404 }, 404));
          return;
        }

        if (!hasActiveDeployment) {
          await route.fulfill(
            jsonResponse({ error: "Database has no active deployment yet", status: 409 }, 409)
          );
          return;
        }

        const snapshotName = decodeURIComponent(relativePath.split("/backups/snapshots/")[1] ?? "");
        databaseSnapshotsState[databaseId] = (databaseSnapshotsState[databaseId] ?? []).filter(
          (snapshot) => snapshot.name !== snapshotName,
        );
        await route.fulfill(
          jsonResponse({
            success: true,
            message: "Snapshot deleted",
          })
        );
        return;
      }

      if (method === "DELETE") {
        if (!database) {
          await route.fulfill(jsonResponse({ error: "Database not found", status: 404 }, 404));
          return;
        }

        databasesState = databasesState.filter((entry) => entry.id !== databaseId);
        await route.fulfill({ status: 204 });
        return;
      }
    }

    if (pathname === `${apiBase}/apps` && method === "POST") {
      const payload = request.postDataJSON() as
        | {
            name?: string;
            git_url?: string;
            github_installation_id?: number;
            github_repo_id?: number;
            github_repo_full_name?: string;
          }
        | undefined;
      const nextIndex = appsState.length + 1;
      const createdApp = {
        id: `app-${nextIndex}`,
        name: payload?.name ?? `app-${nextIndex}`,
        git_url: payload?.git_url ?? "https://github.com/mikrom/new-app",
        port: 3000,
        hostname: null,
        github_webhook_secret: null,
        github_installation_id: payload?.github_installation_id,
        github_repo_id: payload?.github_repo_id,
        github_repo_full_name: payload?.github_repo_full_name,
        active_deployment_id: null,
        desired_replicas: 1,
        min_replicas: 1,
        max_replicas: 1,
        autoscaling_enabled: false,
        cpu_threshold: 80,
        mem_threshold: 80,
        scale_state: "active",
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
      };
      appsState = [createdApp, ...appsState];
      await route.fulfill(jsonResponse(createdApp, 201));
      return;
    }

    if (pathname.startsWith(`${apiBase}/apps/`) && pathname.endsWith("/deployments") && method === "GET") {
      const appName = decodeURIComponent(
        pathname.slice(`${apiBase}/apps/`.length, -"/deployments".length),
      );
      const app = appsState.find((entry) => entry.name === appName);
      await route.fulfill(jsonResponse(app ? appDeployments.filter((deployment) => deployment.app_id === app.id) : []));
      return;
    }

    if (pathname === `${apiBase}/apps/starter/deploy` && method === "POST") {
      await route.fulfill(
        jsonResponse({
          status: "scheduled",
          message: "Deployment queued",
          job_id: "job-3",
          deployment_id: "deploy-3",
          image_tag: "ghcr.io/mikrom/starter:latest",
        })
      );
      return;
    }

    if (pathname === `${apiBase}/apps/starter/deployments/deploy-2/activate` && method === "POST") {
      await route.fulfill(jsonResponse({ success: true }));
      return;
    }

    if (pathname.startsWith(`${apiBase}/apps/`) && pathname.endsWith("/secret") && method === "GET") {
      await route.fulfill(
        jsonResponse({
          github_webhook_secret: options.githubWebhookSecret ?? "secret-123",
        }),
      );
      return;
    }

    if (pathname === `${apiBase}/apps/starter` && method === "DELETE") {
      appsState = appsState.filter((app) => app.name !== "starter");
      await route.fulfill(jsonResponse({ success: true }));
      return;
    }

    if (pathname === `${apiBase}/github/repos` && method === "GET") {
      await route.fulfill(jsonResponse([]));
      return;
    }

    if (pathname === `${apiBase}/github/install` && method === "GET") {
      await route.fulfill(jsonResponse({ url: "https://github.com/apps/mikrom/installations/new" }));
      return;
    }

    if (pathname === `${apiBase}/health` && method === "GET") {
      await route.fulfill(jsonResponse({
        status: "ok",
        version: "1.0.0",
        services: {
          API: "ONLINE",
          Agents: "ONLINE",
          Scheduler: "ONLINE",
          Builder: "ONLINE",
          Router: "ONLINE",
        },
      }));
      return;
    }

    if (pathname === `${apiBase}/workspace/events` && method === "GET") {
      await route.fulfill(sseResponse());
      return;
    }

    if (pathname === `${apiBase}/deployments/events` && method === "GET") {
      await route.fulfill(sseResponse());
      return;
    }

    if (pathname === `${apiBase}/deployments/active` && method === "GET") {
      await route.fulfill(jsonResponse(deployments));
      return;
    }

    if (pathname === `${apiBase}/networking/mesh` && method === "GET") {
      await route.fulfill(jsonResponse(mesh));
      return;
    }

    if (pathname === `${apiBase}/networking/mesh/stream` && method === "GET") {
      await route.fulfill(sseResponse());
      return;
    }

    if (pathname === `${apiBase}/volumes` && method === "GET") {
      await route.fulfill(jsonResponse(volumesState));
      return;
    }

    if (pathname.startsWith(`${apiBase}/volumes/`) && method === "DELETE") {
      const volumeId = decodeURIComponent(pathname.slice(`${apiBase}/volumes/`.length));
      const volume = volumesState.find((entry) => entry.id === volumeId);
      if (!volume) {
        await route.fulfill(jsonResponse({ error: "Volume not found", status: 404 }, 404));
        return;
      }

      volumesState = volumesState.filter((entry) => entry.id !== volumeId);
      await route.fulfill({ status: 204 });
      return;
    }

    if (pathname === `${apiBase}/volumes/vol-1/snapshots` && method === "GET") {
      await route.fulfill(jsonResponse(volumeSnapshots));
      return;
    }

    if (pathname.startsWith(`${apiBase}/apps/`) && pathname.endsWith("/metrics/stream") && method === "GET") {
      await route.fulfill(sseResponse());
      return;
    }

    if (pathname.startsWith(`${apiBase}/apps/`) && pathname.endsWith("/logs/stream") && method === "GET") {
      await route.fulfill(sseResponse());
      return;
    }

    if (pathname.startsWith(`${apiBase}/apps/`) && pathname.endsWith("/security-groups") && method === "GET") {
      const appName = decodeURIComponent(
        pathname.slice(`${apiBase}/apps/`.length, -"/security-groups".length),
      );
      const app = appsState.find((entry) => entry.name === appName);
      await route.fulfill(jsonResponse(app && app.name === "starter" ? securityRulesState : []));
      return;
    }

    if (pathname === `${apiBase}/apps/starter/security-groups` && method === "POST") {
      const payload = request.postDataJSON() as
        | {
            protocol?: string;
            port_start?: number;
            port_end?: number;
            action?: string;
          }
        | undefined;
      const createdRule = {
        id: "rule-2",
        app_id: "app-1",
        protocol: payload?.protocol ?? "tcp",
        port_start: payload?.port_start ?? 8080,
        port_end: payload?.port_end ?? 8080,
        action: payload?.action ?? "allow",
        priority: 90,
        created_at: new Date().toISOString(),
      };
      securityRulesState = [createdRule, ...securityRulesState];
      await route.fulfill(jsonResponse(createdRule));
      return;
    }

    throw new Error(`Unhandled API route: ${method} ${pathname}`);
  });
}

export async function expectDashboardReady(page: Page) {
  await expect(page.getByRole("heading", { name: "Dashboard" })).toBeVisible();
}

export { authToken };
