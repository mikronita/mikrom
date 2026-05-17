import { json } from "@sveltejs/kit";
const HOP_BY_HOP_REQUEST_HEADERS = /* @__PURE__ */ new Set([
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
  "upgrade"
]);
const HOP_BY_HOP_RESPONSE_HEADERS = /* @__PURE__ */ new Set([
  "connection",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade"
]);
function getUpstreamBaseUrl() {
  return (process.env.API_UPSTREAM_URL || process.env.PUBLIC_API_URL || "http://localhost:5001").replace(/\/+$/, "");
}
function joinUrl(base, path = "") {
  const normalizedBase = base.replace(/\/+$/, "");
  const normalizedPath = path.replace(/^\/+/, "");
  return normalizedPath ? `${normalizedBase}/${normalizedPath}` : normalizedBase;
}
async function proxyApiRequest(event, path = "") {
  const upstreamUrl = new URL(joinUrl(getUpstreamBaseUrl(), `v1/${path}`));
  upstreamUrl.search = event.url.search;
  const headers = new Headers(event.request.headers);
  for (const header of HOP_BY_HOP_REQUEST_HEADERS) {
    headers.delete(header);
  }
  const init = {
    method: event.request.method,
    headers,
    redirect: "manual"
  };
  if (!["GET", "HEAD"].includes(event.request.method)) {
    init.body = event.request.body;
    init.duplex = "half";
  }
  let upstreamResponse;
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
    headers: responseHeaders
  });
}
const handle = async (event) => proxyApiRequest(event, event.params.path);
const GET = handle;
const POST = handle;
const PUT = handle;
const PATCH = handle;
const DELETE = handle;
const HEAD = handle;
const OPTIONS = handle;
export {
  DELETE,
  GET,
  HEAD,
  OPTIONS,
  PATCH,
  POST,
  PUT
};
