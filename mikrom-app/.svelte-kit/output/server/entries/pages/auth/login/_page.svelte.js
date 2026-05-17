import { a6 as sanitize_props, ae as spread_props, ac as slot, P as head, j as attr } from "../../../../chunks/renderer.js";
import "@sveltejs/kit/internal";
import "../../../../chunks/exports.js";
import "../../../../chunks/utils.js";
import "@sveltejs/kit/internal/server";
import "../../../../chunks/root.js";
import "../../../../chunks/state.svelte.js";
import { I as Icon, C as Card, F as Field, a as Input } from "../../../../chunks/Field.js";
import "../../../../chunks/api.js";
function Box($$renderer, $$props) {
  const $$sanitized_props = sanitize_props($$props);
  /**
   * @license lucide-svelte v0.542.0 - ISC
   *
   * ISC License
   *
   * Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2023 as part of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2025.
   *
   * Permission to use, copy, modify, and/or distribute this software for any
   * purpose with or without fee is hereby granted, provided that the above
   * copyright notice and this permission notice appear in all copies.
   *
   * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
   * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
   * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
   * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
   * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
   * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
   * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
   *
   * ---
   *
   * The MIT License (MIT) (for portions derived from Feather)
   *
   * Copyright (c) 2013-2023 Cole Bemis
   *
   * Permission is hereby granted, free of charge, to any person obtaining a copy
   * of this software and associated documentation files (the "Software"), to deal
   * in the Software without restriction, including without limitation the rights
   * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
   * copies of the Software, and to permit persons to whom the Software is
   * furnished to do so, subject to the following conditions:
   *
   * The above copyright notice and this permission notice shall be included in all
   * copies or substantial portions of the Software.
   *
   * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
   * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
   * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
   * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
   * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
   * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
   * SOFTWARE.
   *
   */
  const iconNode = [
    [
      "path",
      {
        "d": "M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z"
      }
    ],
    ["path", { "d": "m3.3 7 8.7 5 8.7-5" }],
    ["path", { "d": "M12 22V12" }]
  ];
  Icon($$renderer, spread_props([
    { name: "box" },
    $$sanitized_props,
    {
      /**
       * @component @name Box
       * @description Lucide SVG icon component, renders SVG Element with children.
       *
       * @preview ![img](data:image/svg+xml;base64,PHN2ZyAgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIgogIHdpZHRoPSIyNCIKICBoZWlnaHQ9IjI0IgogIHZpZXdCb3g9IjAgMCAyNCAyNCIKICBmaWxsPSJub25lIgogIHN0cm9rZT0iIzAwMCIgc3R5bGU9ImJhY2tncm91bmQtY29sb3I6ICNmZmY7IGJvcmRlci1yYWRpdXM6IDJweCIKICBzdHJva2Utd2lkdGg9IjIiCiAgc3Ryb2tlLWxpbmVjYXA9InJvdW5kIgogIHN0cm9rZS1saW5lam9pbj0icm91bmQiCj4KICA8cGF0aCBkPSJNMjEgOGEyIDIgMCAwIDAtMS0xLjczbC03LTRhMiAyIDAgMCAwLTIgMGwtNyA0QTIgMiAwIDAgMCAzIDh2OGEyIDIgMCAwIDAgMSAxLjczbDcgNGEyIDIgMCAwIDAgMiAwbDctNEEyIDIgMCAwIDAgMjEgMTZaIiAvPgogIDxwYXRoIGQ9Im0zLjMgNyA4LjcgNSA4LjctNSIgLz4KICA8cGF0aCBkPSJNMTIgMjJWMTIiIC8+Cjwvc3ZnPgo=) - https://lucide.dev/icons/box
       * @see https://lucide.dev/guide/packages/lucide-svelte - Documentation
       *
       * @param {Object} props - Lucide icons props and any valid SVG attribute
       * @returns {FunctionalComponent} Svelte component
       *
       */
      iconNode,
      children: ($$renderer2) => {
        $$renderer2.push(`<!--[-->`);
        slot($$renderer2, $$props, "default", {});
        $$renderer2.push(`<!--]-->`);
      },
      $$slots: { default: true }
    }
  ]));
}
function _page($$renderer, $$props) {
  $$renderer.component(($$renderer2) => {
    let email = "";
    let password = "";
    let loading = false;
    let $$settled = true;
    let $$inner_renderer;
    function $$render_inner($$renderer3) {
      head("1i2smtp", $$renderer3, ($$renderer4) => {
        $$renderer4.title(($$renderer5) => {
          $$renderer5.push(`<title>Mikrom - Login</title>`);
        });
      });
      $$renderer3.push(`<div class="flex min-h-screen flex-col bg-background px-4 py-10"><div class="mx-auto flex w-full max-w-md flex-1 flex-col items-center justify-center gap-6"><div class="flex flex-col items-center gap-3 text-center"><div class="flex size-10 items-center justify-center rounded-full border border-border bg-card text-foreground shadow-sm">`);
      Box($$renderer3, { class: "size-5" });
      $$renderer3.push(`<!----></div> <div class="flex flex-col gap-1"><h1 class="text-2xl font-semibold tracking-tight">Sign in to Mikrom</h1> <p class="text-sm text-muted-foreground">Use your account to manage applications and microVMs.</p></div></div> `);
      Card($$renderer3, {
        class: "w-full max-w-md",
        children: ($$renderer4) => {
          $$renderer4.push(`<div class="flex flex-col items-center gap-2.5 border-b border-border px-5 py-5 text-center"><div class="mb-2 flex justify-center"><div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">`);
          Box($$renderer4, { class: "size-4" });
          $$renderer4.push(`<!----></div></div> <div class="text-2xl font-semibold tracking-tight">Welcome back</div> <div class="text-sm text-muted-foreground">Enter your credentials to access your dashboard</div></div> <div class="p-5"><form class="flex flex-col gap-4">`);
          {
            $$renderer4.push("<!--[-1-->");
          }
          $$renderer4.push(`<!--]--> `);
          {
            $$renderer4.push("<!--[-1-->");
          }
          $$renderer4.push(`<!--]--> `);
          Field($$renderer4, {
            label: "Email address",
            forId: "email",
            children: ($$renderer5) => {
              Input($$renderer5, {
                id: "email",
                type: "email",
                placeholder: "name@example.com",
                required: true,
                disabled: loading,
                get value() {
                  return email;
                },
                set value($$value) {
                  email = $$value;
                  $$settled = false;
                }
              });
            },
            $$slots: { default: true }
          });
          $$renderer4.push(`<!----> <div class="flex flex-col gap-1.5"><div class="flex items-center justify-between"><label for="password" class="text-sm font-medium">Password</label> <button type="button" class="text-xs text-muted-foreground transition-colors hover:text-foreground">Forgot password?</button></div> `);
          Input($$renderer4, {
            id: "password",
            type: "password",
            placeholder: "••••••••",
            required: true,
            disabled: loading,
            get value() {
              return password;
            },
            set value($$value) {
              password = $$value;
              $$settled = false;
            }
          });
          $$renderer4.push(`<!----></div> <div class="flex flex-col gap-4 pt-2"><button type="submit"${attr("disabled", loading, true)} class="inline-flex h-9 w-full items-center justify-center gap-2 whitespace-nowrap rounded-md border border-transparent bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50">`);
          {
            $$renderer4.push("<!--[-1-->");
            $$renderer4.push(`Sign In`);
          }
          $$renderer4.push(`<!--]--></button> <div class="text-center text-sm text-muted-foreground">Don't have an account? <a href="/auth/register" class="font-semibold text-foreground hover:underline">Create one for free</a></div></div></form></div>`);
        },
        $$slots: { default: true }
      });
      $$renderer3.push(`<!----> <p class="max-w-sm text-center text-xs leading-5 text-muted-foreground">Protected by your workspace credentials. By continuing, you agree to Mikrom's terms and privacy policy.</p></div></div>`);
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
