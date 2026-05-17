import type { RequestHandler } from "./$types";
import { proxyApiRequest } from "$lib/server/api-proxy";

const handle: RequestHandler = async (event) => proxyApiRequest(event, event.params.path);

export const GET = handle;
export const POST = handle;
export const PUT = handle;
export const PATCH = handle;
export const DELETE = handle;
export const HEAD = handle;
export const OPTIONS = handle;
