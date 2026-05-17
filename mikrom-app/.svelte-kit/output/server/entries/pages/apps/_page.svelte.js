import { P as head } from "../../../chunks/renderer.js";
import { s as subscribeVms, o as onDestroy } from "../../../chunks/vms.js";
import { D as DashboardLayout } from "../../../chunks/DashboardLayout.js";
import "@sveltejs/kit/internal";
import "../../../chunks/exports.js";
import "../../../chunks/utils.js";
import "@sveltejs/kit/internal/server";
import "../../../chunks/root.js";
import "../../../chunks/state.svelte.js";
import "../../../chunks/api.js";
function _page($$renderer, $$props) {
  $$renderer.component(($$renderer2) => {
    const unsubscribe = subscribeVms((next) => {
    });
    onDestroy(() => unsubscribe());
    let $$settled = true;
    let $$inner_renderer;
    function $$render_inner($$renderer3) {
      head("12ewbr5", $$renderer3, ($$renderer4) => {
        $$renderer4.title(($$renderer5) => {
          $$renderer5.push(`<title>Mikrom - Applications</title>`);
        });
      });
      DashboardLayout($$renderer3);
    }
    do {
      $$settled = true;
      $$inner_renderer = $$renderer2.copy();
      $$render_inner($$inner_renderer);
    } while (!$$settled);
    $$renderer2.subsume($$inner_renderer);
  });
}
export {
  _page as default
};
