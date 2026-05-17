import{d as Q,a as f,h as T,i as h,s as V,t as W}from"../chunks/CFMDmYNZ.js";import{b as X,a as ee,h as re}from"../chunks/BIhFgwN4.js";import{ag as z,aU as ae,aR as te,ad as se,Z as o,b1 as t,aj as e,bh as U,b9 as u,$ as oe,aY as i,aO as b,aM as p,bi as ie}from"../chunks/eeULnnp2.js";import{l as le,s as de,i as B}from"../chunks/j9I5wVj4.js";import{p as ne}from"../chunks/CWmzcjye.js";import{g as ce}from"../chunks/BR566eKR.js";import{I as ue,C as fe,a as I,J as me}from"../chunks/CLUnmxSh.js";import{F as j}from"../chunks/Dx-teoMC.js";import{A as pe}from"../chunks/CCVFB79h.js";import{C as ve}from"../chunks/kr5jEozg.js";import{L as ge}from"../chunks/DC9ePOvb.js";function xe($,v){const c=le(v,["children","$$slots","$$events","$$legacy"]);/**
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
 */const l=[["path",{d:"M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"}],["circle",{cx:"9",cy:"7",r:"4"}],["line",{x1:"19",x2:"19",y1:"8",y2:"14"}],["line",{x1:"22",x2:"16",y1:"11",y2:"11"}]];ue($,de({name:"user-plus"},()=>c,{get iconNode(){return l},children:(m,d)=>{var a=Q(),_=z(a);X(_,v,"default",{}),f(m,a)},$$slots:{default:!0}}))}var be=h("<!> <div> </div>",1),he=h("<!> Creating account...",1),$e=h('<div class="p-5 pt-5"><form class="flex flex-col gap-4"><!> <!> <!> <!> <div class="flex flex-col gap-4 pt-2"><button type="submit" class="inline-flex h-9 w-full items-center justify-center gap-2 whitespace-nowrap rounded-md border border-transparent bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"><!></button> <div class="text-center text-sm text-muted-foreground">Already have an account? <a href="/auth/login" class="font-medium text-foreground hover:underline">Sign in</a></div></div></form></div>'),_e=h('<div class="flex min-h-screen flex-col bg-background px-4 py-10"><div class="mx-auto flex w-full max-w-md flex-1 flex-col items-center justify-center gap-6"><div class="flex flex-col items-center gap-3 text-center"><div class="flex size-10 items-center justify-center rounded-full border border-border bg-card text-foreground shadow-sm"><!></div> <div class="flex flex-col gap-1"><h1 class="text-2xl font-semibold tracking-tight">Create your Mikrom account</h1> <p class="text-sm text-muted-foreground">Set up access to deploy and manage your applications.</p></div></div> <!> <p class="max-w-sm text-center text-xs leading-5 text-muted-foreground">By continuing, you agree to Mikrom&apos;s terms and privacy policy.</p></div></div>');function Re($,v){ae(v,!1);let c=p(""),l=p(""),m=p(""),d=p(""),a=p(!1);async function _(g){if(g.preventDefault(),t(d,""),!e(c)||!e(l)){t(d,"Email and password are required");return}if(e(l).length<8){t(d,"Password must be at least 8 characters");return}if(e(l)!==e(m)){t(d,"Passwords do not match");return}t(a,!0);const x=await me({email:e(c),password:e(l)});if(t(a,!1),x.error){t(d,x.error);return}x.data&&(await ie(),await ce("/auth/login?registered=true"))}ee();var y=_e();re("8bdjn9",g=>{se(()=>{oe.title="Mikrom - Register"})});var M=o(y),w=o(M),q=o(w),H=o(q);xe(H,{class:"size-5"}),i(q),b(2),i(w);var J=u(w,2);fe(J,{class:"w-full",children:(g,x)=>{var P=$e(),C=o(P),A=o(C);{var O=r=>{pe(r,{variant:"destructive",children:(s,n)=>{var F=be(),L=z(F);ve(L,{class:"size-4"});var N=u(L,2),K=o(N,!0);i(N),U(()=>V(K,e(d))),f(s,F)},$$slots:{default:!0}})};B(A,r=>{e(d)&&r(O)})}var R=u(A,2);j(R,{label:"Email address",forId:"email",children:(r,s)=>{I(r,{id:"email",type:"email",placeholder:"name@example.com",required:!0,get disabled(){return e(a)},get value(){return e(c)},set value(n){t(c,n)},$$legacy:!0})},$$slots:{default:!0}});var S=u(R,2);j(S,{label:"Password",forId:"password",children:(r,s)=>{I(r,{id:"password",type:"password",placeholder:"At least 8 characters",required:!0,get disabled(){return e(a)},get value(){return e(l)},set value(n){t(l,n)},$$legacy:!0})},$$slots:{default:!0}});var D=u(S,2);j(D,{label:"Confirm Password",forId:"confirmPassword",children:(r,s)=>{I(r,{id:"confirmPassword",type:"password",placeholder:"Repeat your password",required:!0,get disabled(){return e(a)},get value(){return e(m)},set value(n){t(m,n)},$$legacy:!0})},$$slots:{default:!0}});var E=u(D,2),k=o(E),Y=o(k);{var Z=r=>{var s=he(),n=z(s);ge(n,{class:"size-4 animate-spin"}),b(),f(r,s)},G=r=>{var s=W("Create account");f(r,s)};B(Y,r=>{e(a)?r(Z):r(G,-1)})}i(k),b(2),i(E),i(C),i(P),U(()=>k.disabled=e(a)),T("submit",C,ne(_)),f(g,P)},$$slots:{default:!0}}),b(2),i(M),i(y),f($,y),te()}export{Re as component};
