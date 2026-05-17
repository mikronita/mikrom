import{d as ae,a as n,i as _,h as se,s as oe,t as ie}from"../chunks/CFMDmYNZ.js";import{b as de,a as le,h as ne}from"../chunks/BIhFgwN4.js";import{a as ce}from"../chunks/BnAFb8q8.js";import{ag as b,aU as me,aR as ve,ad as fe,Z as r,b1 as o,$ as ue,aY as t,aO as v,b9 as i,aj as a,bh as Y,aM as x,bi as pe}from"../chunks/eeULnnp2.js";import{l as ge,s as xe,i as I}from"../chunks/j9I5wVj4.js";import{p as be}from"../chunks/CWmzcjye.js";import{g as _e}from"../chunks/BR566eKR.js";import{I as he,C as $e,a as Z,G as ye,L as we}from"../chunks/CLUnmxSh.js";import{F as ke}from"../chunks/Dx-teoMC.js";import{A as G}from"../chunks/CCVFB79h.js";import{C as ze}from"../chunks/CAwfo1-N.js";import{C as Me}from"../chunks/kr5jEozg.js";import{L as Pe}from"../chunks/DC9ePOvb.js";function O(w,h){const c=ge(h,["children","$$slots","$$events","$$legacy"]);/**
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
 */const m=[["path",{d:"M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z"}],["path",{d:"m3.3 7 8.7 5 8.7-5"}],["path",{d:"M12 22V12"}]];he(w,xe({name:"box"},()=>c,{get iconNode(){return m},children:(d,l)=>{var f=ae(),k=b(f);de(k,h,"default",{}),n(d,f)},$$slots:{default:!0}}))}var je=_("<!> <div>Account created! You can now sign in.</div>",1),Ce=_("<!> <div> </div>",1),Ae=_("<!> Signing in...",1),Ie=_('<div class="flex flex-col items-center gap-2.5 border-b border-border px-5 py-5 text-center"><div class="mb-2 flex justify-center"><div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground"><!></div></div> <div class="text-2xl font-semibold tracking-tight">Welcome back</div> <div class="text-sm text-muted-foreground">Enter your credentials to access your dashboard</div></div> <div class="p-5"><form class="flex flex-col gap-4"><!> <!> <!> <div class="flex flex-col gap-1.5"><div class="flex items-center justify-between"><label for="password" class="text-sm font-medium">Password</label> <button type="button" class="text-xs text-muted-foreground transition-colors hover:text-foreground">Forgot password?</button></div> <!></div> <div class="flex flex-col gap-4 pt-2"><button type="submit" class="inline-flex h-9 w-full items-center justify-center gap-2 whitespace-nowrap rounded-md border border-transparent bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"><!></button> <div class="text-center text-sm text-muted-foreground">Don&apos;t have an account? <a href="/auth/register" class="font-semibold text-foreground hover:underline">Create one for free</a></div></div></form></div>',1),Le=_('<div class="flex min-h-screen flex-col bg-background px-4 py-10"><div class="mx-auto flex w-full max-w-md flex-1 flex-col items-center justify-center gap-6"><div class="flex flex-col items-center gap-3 text-center"><div class="flex size-10 items-center justify-center rounded-full border border-border bg-card text-foreground shadow-sm"><!></div> <div class="flex flex-col gap-1"><h1 class="text-2xl font-semibold tracking-tight">Sign in to Mikrom</h1> <p class="text-sm text-muted-foreground">Use your account to manage applications and microVMs.</p></div></div> <!> <p class="max-w-sm text-center text-xs leading-5 text-muted-foreground">Protected by your workspace credentials. By continuing, you agree to Mikrom&apos;s terms and privacy policy.</p></div></div>');function Oe(w,h){me(h,!1);let c=x(""),m=x(""),d=x(""),l=x(!1),f=x(!1);ce(()=>{o(f,new URLSearchParams(window.location.search).get("registered")==="true")});async function k($){if($.preventDefault(),o(d,""),!a(c)||!a(m)){o(d,"Email and password are required");return}o(l,!0);const u=await ye({email:a(c),password:a(m)});if(o(l,!1),u.error){o(d,u.error);return}u.data&&(we(u.data.token),await pe(),await _e("/"))}le();var z=Le();ne("1i2smtp",$=>{fe(()=>{ue.title="Mikrom - Login"})});var L=r(z),M=r(L),S=r(M),T=r(S);O(T,{class:"size-5"}),t(S),v(2),t(M);var W=i(M,2);$e(W,{class:"w-full max-w-md",children:($,u)=>{var q=Ie(),P=b(q),D=r(P),E=r(D),H=r(E);O(H,{class:"size-4"}),t(E),t(D),v(4),t(P);var F=i(P,2),j=r(F),U=r(j);{var J=e=>{G(e,{children:(s,p)=>{var g=je(),y=b(g);ze(y,{class:"size-4"}),v(2),n(s,g)},$$slots:{default:!0}})};I(U,e=>{a(f)&&e(J)})}var B=i(U,2);{var K=e=>{G(e,{variant:"destructive",children:(s,p)=>{var g=Ce(),y=b(g);Me(y,{class:"size-4"});var V=i(y,2),te=r(V,!0);t(V),Y(()=>oe(te,a(d))),n(s,g)},$$slots:{default:!0}})};I(B,e=>{a(d)&&e(K)})}var N=i(B,2);ke(N,{label:"Email address",forId:"email",children:(e,s)=>{Z(e,{id:"email",type:"email",placeholder:"name@example.com",required:!0,get disabled(){return a(l)},get value(){return a(c)},set value(p){o(c,p)},$$legacy:!0})},$$slots:{default:!0}});var C=i(N,2),Q=i(r(C),2);Z(Q,{id:"password",type:"password",placeholder:"••••••••",required:!0,get disabled(){return a(l)},get value(){return a(m)},set value(e){o(m,e)},$$legacy:!0}),t(C);var R=i(C,2),A=r(R),X=r(A);{var ee=e=>{var s=Ae(),p=b(s);Pe(p,{class:"size-4 animate-spin"}),v(),n(e,s)},re=e=>{var s=ie("Sign In");n(e,s)};I(X,e=>{a(l)?e(ee):e(re,-1)})}t(A),v(2),t(R),t(j),t(F),Y(()=>A.disabled=a(l)),se("submit",j,be(k)),n($,q)},$$slots:{default:!0}}),v(2),t(L),t(z),n(w,z),ve()}export{Oe as component};
