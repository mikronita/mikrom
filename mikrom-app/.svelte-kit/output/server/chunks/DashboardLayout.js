import { K as getContext, ag as store_get, aj as unsubscribe_stores } from "./renderer.js";
import "@sveltejs/kit/internal";
import "./exports.js";
import "./utils.js";
import "@sveltejs/kit/internal/server";
import "./root.js";
import "./state.svelte.js";
import { w as writable } from "./index.js";
import "./api.js";
const getStores = () => {
  const stores$1 = getContext("__svelte__");
  return {
    /** @type {typeof page} */
    page: {
      subscribe: stores$1.page.subscribe
    },
    /** @type {typeof navigating} */
    navigating: {
      subscribe: stores$1.navigating.subscribe
    },
    /** @type {typeof updated} */
    updated: stores$1.updated
  };
};
const page = {
  subscribe(fn) {
    const store = getStores().page;
    return store.subscribe(fn);
  }
};
function AuthGuard($$renderer, $$props) {
  $$renderer.component(($$renderer2) => {
    {
      $$renderer2.push("<!--[0-->");
      $$renderer2.push(`<div class="flex min-h-screen items-center justify-center bg-background text-muted-foreground"><div class="flex items-center gap-3 rounded-md border bg-card px-4 py-3 shadow-sm"><div class="size-3 animate-pulse rounded-full bg-primary"></div> <span>Loading workspace...</span></div></div>`);
    }
    $$renderer2.push(`<!--]-->`);
  });
}
function readCachedProfile() {
  return null;
}
const { subscribe, set } = writable(readCachedProfile());
function DashboardLayout($$renderer, $$props) {
  $$renderer.component(($$renderer2) => {
    var $$store_subs;
    store_get($$store_subs ??= {}, "$page", page).url.pathname.split("/").filter(Boolean);
    let $$settled = true;
    let $$inner_renderer;
    function $$render_inner($$renderer3) {
      AuthGuard($$renderer3);
    }
    do {
      $$settled = true;
      $$inner_renderer = $$renderer2.copy();
      $$render_inner($$inner_renderer);
    } while (!$$settled);
    $$renderer2.subsume($$inner_renderer);
    if ($$store_subs) unsubscribe_stores($$store_subs);
  });
}
export {
  DashboardLayout as D,
  page as p
};
