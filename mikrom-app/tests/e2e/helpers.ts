import { expect, type Page } from "@playwright/test";
import {
  appDeployments,
  apps,
  authToken,
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
  let securityRulesState = [...securityRules];
  let appsState = [...apps];

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
      await route.fulfill(jsonResponse(projects));
      return;
    }

    if (pathname === `${apiBase}/apps` && method === "GET") {
      await route.fulfill(jsonResponse(appsState));
      return;
    }

    if (pathname === `${apiBase}/apps/starter/deployments` && method === "GET") {
      await route.fulfill(jsonResponse(appDeployments));
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

    if (pathname === `${apiBase}/apps/starter/secret` && method === "GET") {
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

    if (pathname === `${apiBase}/apps` && method === "POST") {
      await route.fulfill(jsonResponse({ id: "app-2" }));
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

    if (pathname === `${apiBase}/deployments/active` && method === "GET") {
      await route.fulfill(jsonResponse(deployments));
      return;
    }

    if (pathname === `${apiBase}/networking/mesh` && method === "GET") {
      await route.fulfill(jsonResponse(mesh));
      return;
    }

    if (pathname === `${apiBase}/volumes` && method === "GET") {
      await route.fulfill(jsonResponse(volumes));
      return;
    }

    if (pathname === `${apiBase}/volumes/vol-1/snapshots` && method === "GET") {
      await route.fulfill(jsonResponse(volumeSnapshots));
      return;
    }

    if (pathname === `${apiBase}/apps/starter/security-groups` && method === "GET") {
      await route.fulfill(jsonResponse(securityRulesState));
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
