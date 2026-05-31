# mikrom-app

SvelteKit migration of the Mikrom control plane dashboard.

## Stack

- SvelteKit
- pnpm
- Tailwind CSS 4
- Lucide icons

## Environment

- `API_UPSTREAM_URL`: URL interna del backend REST, por ejemplo `http://localhost:5001`
- `PUBLIC_APP_URL`: URL pública del dashboard, por ejemplo `http://localhost:5173`

## Scripts

```bash
pnpm install
pnpm dev
pnpm check
pnpm build
pnpm test
pnpm test:e2e
```

## Testing

- `pnpm test` runs Vitest in watch mode for local development.
- `pnpm test:unit` runs the unit suite once.
- `pnpm test:coverage` generates coverage reports.
- `pnpm test:e2e` runs Playwright against the local SvelteKit app.
