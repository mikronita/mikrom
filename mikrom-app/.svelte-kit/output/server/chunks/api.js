import { p as public_env } from "./shared-server.js";
(public_env.PUBLIC_APP_URL || "http://localhost:5173").replace(/\/+$/, "");
