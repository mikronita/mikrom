import { af as ssr_context } from "./renderer.js";
import "./api.js";
function onDestroy(fn) {
  /** @type {SSRContext} */
  ssr_context.r.on_destroy(fn);
}
const callbacks = /* @__PURE__ */ new Set();
let current = [];
function getCurrentVms() {
  return current;
}
function subscribeVms(cb) {
  callbacks.add(cb);
  cb(current);
  return () => callbacks.delete(cb);
}
export {
  getCurrentVms as g,
  onDestroy as o,
  subscribeVms as s
};
