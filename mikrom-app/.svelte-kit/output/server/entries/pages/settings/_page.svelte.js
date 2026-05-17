import { P as head } from "../../../chunks/renderer.js";
import { D as DashboardLayout } from "../../../chunks/DashboardLayout.js";
import "../../../chunks/api.js";
function _page($$renderer, $$props) {
  $$renderer.component(($$renderer2) => {
    let $$settled = true;
    let $$inner_renderer;
    function $$render_inner($$renderer3) {
      head("1i19ct2", $$renderer3, ($$renderer4) => {
        $$renderer4.title(($$renderer5) => {
          $$renderer5.push(`<title>Mikrom - Settings</title>`);
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
