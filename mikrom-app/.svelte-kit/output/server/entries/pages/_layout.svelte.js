import { D as ensure_array_like, ag as store_get, k as attr_class, F as escape_html, aj as unsubscribe_stores, P as head, ac as slot } from "../../chunks/renderer.js";
import { w as writable } from "../../chunks/index.js";
const toasts = writable([]);
function ToastViewport($$renderer, $$props) {
  $$renderer.component(($$renderer2) => {
    var $$store_subs;
    $$renderer2.push(`<div class="fixed right-3 top-3 z-[60] flex w-[calc(100vw-1.5rem)] max-w-sm flex-col gap-2 sm:w-96"><!--[-->`);
    const each_array = ensure_array_like(store_get($$store_subs ??= {}, "$toasts", toasts));
    for (let $$index = 0, $$length = each_array.length; $$index < $$length; $$index++) {
      let toast = each_array[$$index];
      $$renderer2.push(`<div${attr_class(`rounded-md border px-4 py-3 shadow-lg ${toast.variant === "error" ? "border-destructive/30 bg-destructive/10 text-destructive" : toast.variant === "success" ? "border-status-online/30 bg-[color:color-mix(in_srgb,var(--status-online)_12%,white)] text-[var(--status-online)]" : "border-border bg-card text-foreground"}`)}><div class="flex items-start justify-between gap-3"><p class="text-sm">${escape_html(toast.message)}</p> <button class="text-xs text-muted-foreground hover:text-foreground">x</button></div></div>`);
    }
    $$renderer2.push(`<!--]--></div>`);
    if ($$store_subs) unsubscribe_stores($$store_subs);
  });
}
const THEME_KEY = "mikrom_theme";
function applyTheme(theme) {
  if (typeof document === "undefined") return;
  document.documentElement.classList.toggle("dark", theme === "dark");
  document.documentElement.classList.toggle("light-theme", theme === "light");
  document.documentElement.style.colorScheme = theme;
  window.dispatchEvent(new CustomEvent("mikrom-theme-change", { detail: theme }));
}
function initTheme() {
  if (typeof window === "undefined") return;
  const stored = localStorage.getItem(THEME_KEY);
  const prefersDark = window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
  const next = stored === "light" || stored === "dark" ? stored : prefersDark ? "dark" : "light";
  applyTheme(next);
  localStorage.setItem(THEME_KEY, next);
}
function _layout($$renderer, $$props) {
  $$renderer.component(($$renderer2) => {
    initTheme();
    head("12qhfyh", $$renderer2, ($$renderer3) => {
      $$renderer3.title(($$renderer4) => {
        $$renderer4.push(`<title>Mikrom</title>`);
      });
      $$renderer3.push(`<meta name="description" content="Micromobility management platform"/>`);
    });
    $$renderer2.push(`<!--[-->`);
    slot($$renderer2, $$props, "default", {});
    $$renderer2.push(`<!--]--> `);
    ToastViewport($$renderer2);
    $$renderer2.push(`<!---->`);
  });
}
export {
  _layout as default
};
