import { ag as store_get, P as head, aj as unsubscribe_stores, F as escape_html } from "../../../../chunks/renderer.js";
import { D as DashboardLayout, p as page } from "../../../../chunks/DashboardLayout.js";
import "@sveltejs/kit/internal";
import "../../../../chunks/exports.js";
import "../../../../chunks/utils.js";
import "@sveltejs/kit/internal/server";
import "../../../../chunks/root.js";
import "../../../../chunks/state.svelte.js";
import "../../../../chunks/api.js";
function _page($$renderer, $$props) {
  $$renderer.component(($$renderer2) => {
    var $$store_subs;
    const appName = decodeURIComponent(store_get($$store_subs ??= {}, "$page", page).params.appName ?? "");
    head("19by34m", $$renderer2, ($$renderer3) => {
      $$renderer3.title(($$renderer4) => {
        $$renderer4.push(`<title>Mikrom - ${escape_html(appName)}</title>`);
      });
    });
    DashboardLayout($$renderer2);
    if ($$store_subs) unsubscribe_stores($$store_subs);
  });
}
export {
  _page as default
};
