import { json, type RequestEvent } from "@sveltejs/kit";

const HOP_BY_HOP_REQUEST_HEADERS = new Set([
  "connection",
  "content-length",
  "expect",
  "host",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade",
]);

const HOP_BY_HOP_RESPONSE_HEADERS = new Set([
  "connection",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade",
]);

function getUpstreamBaseUrl() {
  return (process.env.API_UPSTREAM_URL || process.env.PUBLIC_API_URL || "http://localhost:5001").replace(/\/+$/, "");
}

function joinUrl(base: string, path = "") {
  const normalizedBase = base.replace(/\/+$/, "");
  const normalizedPath = path.replace(/^\/+/, "");
  return normalizedPath ? `${normalizedBase}/${normalizedPath}` : normalizedBase;
}

function readCookie(header: string | null, name: string) {
  if (!header) return null;

  for (const part of header.split(";")) {
    const [rawKey, ...rawValue] = part.trim().split("=");
    if (rawKey === name) {
      return decodeURIComponent(rawValue.join("="));
    }
  }

  return null;
}

export async function proxyApiRequest(event: RequestEvent, path = "") {
  const upstreamUrl = new URL(joinUrl(getUpstreamBaseUrl(), `v1/${path}`));
  upstreamUrl.search = event.url.search;

  const headers = new Headers(event.request.headers);
  const tenantId = readCookie(event.request.headers.get("cookie"), "mikrom_active_project");
  for (const header of HOP_BY_HOP_REQUEST_HEADERS) {
    headers.delete(header);
  }
  if (tenantId) {
    headers.set("x-mikrom-tenant-id", tenantId);
  }

  const init: RequestInit & { duplex?: "half" } = {
    method: event.request.method,
    headers,
    redirect: "manual",
  };

  if (!["GET", "HEAD"].includes(event.request.method)) {
    init.body = event.request.body;
    init.duplex = "half";
  }

  let upstreamResponse: Response;
  try {
    upstreamResponse = await fetch(upstreamUrl, init);
  } catch {
    return json({ error: "API upstream unreachable" }, { status: 502 });
  }

  const responseHeaders = new Headers(upstreamResponse.headers);
  for (const header of HOP_BY_HOP_RESPONSE_HEADERS) {
    responseHeaders.delete(header);
  }

  return new Response(upstreamResponse.body, {
    status: upstreamResponse.status,
    statusText: upstreamResponse.statusText,
    headers: responseHeaders,
  });
}
