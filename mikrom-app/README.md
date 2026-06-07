# mikrom-app

`mikrom-app` is the Mikrom dashboard. It is a SvelteKit application built with Svelte 5, Vite, Tailwind CSS 4, shadcn-svelte, and bits-ui.

## Stack

- SvelteKit
- Svelte 5
- Tailwind CSS 4
- shadcn-svelte
- bits-ui
- Lucide icons
- Vitest
- Playwright

## Scripts

```bash
pnpm install
pnpm dev
pnpm check
pnpm lint
pnpm build
pnpm test:unit
pnpm test:e2e
```

## Environment

- `API_UPSTREAM_URL`: backend REST API URL, for example `http://localhost:5001`
- `PUBLIC_APP_URL`: public dashboard URL, for example `http://localhost:5173`
- Billing redirects are served by `mikrom-api`; configure Polar there with `POLAR_ACCESS_TOKEN`, `POLAR_WEBHOOK_SECRET`, and `POLAR_CHECKOUT_PRODUCT_ID` when needed.

## Notes

- Use the existing UI primitives in `src/lib/components/ui` for standard controls.
- The dashboard is validated through the Dagger `ci-smoke`, `ci-fast`, and `app-e2e` flows.
- `components.json` is configured for shadcn-svelte, not the React shadcn registry.
