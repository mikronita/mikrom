import { a6 as sanitize_props, ae as spread_props, ac as slot, P as head, j as attr } from "../../../../chunks/renderer.js";
import "@sveltejs/kit/internal";
import "../../../../chunks/exports.js";
import "../../../../chunks/utils.js";
import "@sveltejs/kit/internal/server";
import "../../../../chunks/root.js";
import "../../../../chunks/state.svelte.js";
import { I as Icon, C as Card, F as Field, a as Input } from "../../../../chunks/Field.js";
import "../../../../chunks/api.js";
function User_plus($$renderer, $$props) {
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
    ["path", { "d": "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" }],
    ["circle", { "cx": "9", "cy": "7", "r": "4" }],
    ["line", { "x1": "19", "x2": "19", "y1": "8", "y2": "14" }],
    ["line", { "x1": "22", "x2": "16", "y1": "11", "y2": "11" }]
  ];
  Icon($$renderer, spread_props([
    { name: "user-plus" },
    $$sanitized_props,
    {
      /**
       * @component @name UserPlus
       * @description Lucide SVG icon component, renders SVG Element with children.
       *
       * @preview ![img](data:image/svg+xml;base64,PHN2ZyAgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIgogIHdpZHRoPSIyNCIKICBoZWlnaHQ9IjI0IgogIHZpZXdCb3g9IjAgMCAyNCAyNCIKICBmaWxsPSJub25lIgogIHN0cm9rZT0iIzAwMCIgc3R5bGU9ImJhY2tncm91bmQtY29sb3I6ICNmZmY7IGJvcmRlci1yYWRpdXM6IDJweCIKICBzdHJva2Utd2lkdGg9IjIiCiAgc3Ryb2tlLWxpbmVjYXA9InJvdW5kIgogIHN0cm9rZS1saW5lam9pbj0icm91bmQiCj4KICA8cGF0aCBkPSJNMTYgMjF2LTJhNCA0IDAgMCAwLTQtNEg2YTQgNCAwIDAgMC00IDR2MiIgLz4KICA8Y2lyY2xlIGN4PSI5IiBjeT0iNyIgcj0iNCIgLz4KICA8bGluZSB4MT0iMTkiIHgyPSIxOSIgeTE9IjgiIHkyPSIxNCIgLz4KICA8bGluZSB4MT0iMjIiIHgyPSIxNiIgeTE9IjExIiB5Mj0iMTEiIC8+Cjwvc3ZnPgo=) - https://lucide.dev/icons/user-plus
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
    let confirmPassword = "";
    let loading = false;
    let $$settled = true;
    let $$inner_renderer;
    function $$render_inner($$renderer3) {
      head("8bdjn9", $$renderer3, ($$renderer4) => {
        $$renderer4.title(($$renderer5) => {
          $$renderer5.push(`<title>Mikrom - Register</title>`);
        });
      });
      $$renderer3.push(`<div class="flex min-h-screen flex-col bg-background px-4 py-10"><div class="mx-auto flex w-full max-w-md flex-1 flex-col items-center justify-center gap-6"><div class="flex flex-col items-center gap-3 text-center"><div class="flex size-10 items-center justify-center rounded-full border border-border bg-card text-foreground shadow-sm">`);
      User_plus($$renderer3, { class: "size-5" });
      $$renderer3.push(`<!----></div> <div class="flex flex-col gap-1"><h1 class="text-2xl font-semibold tracking-tight">Create your Mikrom account</h1> <p class="text-sm text-muted-foreground">Set up access to deploy and manage your applications.</p></div></div> `);
      Card($$renderer3, {
        class: "w-full",
        children: ($$renderer4) => {
          $$renderer4.push(`<div class="p-5 pt-5"><form class="flex flex-col gap-4">`);
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
          $$renderer4.push(`<!----> `);
          Field($$renderer4, {
            label: "Password",
            forId: "password",
            children: ($$renderer5) => {
              Input($$renderer5, {
                id: "password",
                type: "password",
                placeholder: "At least 8 characters",
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
            },
            $$slots: { default: true }
          });
          $$renderer4.push(`<!----> `);
          Field($$renderer4, {
            label: "Confirm Password",
            forId: "confirmPassword",
            children: ($$renderer5) => {
              Input($$renderer5, {
                id: "confirmPassword",
                type: "password",
                placeholder: "Repeat your password",
                required: true,
                disabled: loading,
                get value() {
                  return confirmPassword;
                },
                set value($$value) {
                  confirmPassword = $$value;
                  $$settled = false;
                }
              });
            },
            $$slots: { default: true }
          });
          $$renderer4.push(`<!----> <div class="flex flex-col gap-4 pt-2"><button type="submit"${attr("disabled", loading, true)} class="inline-flex h-9 w-full items-center justify-center gap-2 whitespace-nowrap rounded-md border border-transparent bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50">`);
          {
            $$renderer4.push("<!--[-1-->");
            $$renderer4.push(`Create account`);
          }
          $$renderer4.push(`<!--]--></button> <div class="text-center text-sm text-muted-foreground">Already have an account? <a href="/auth/login" class="font-medium text-foreground hover:underline">Sign in</a></div></div></form></div>`);
        },
        $$slots: { default: true }
      });
      $$renderer3.push(`<!----> <p class="max-w-sm text-center text-xs leading-5 text-muted-foreground">By continuing, you agree to Mikrom's terms and privacy policy.</p></div></div>`);
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
