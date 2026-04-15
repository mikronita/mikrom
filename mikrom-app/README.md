# mikrom-app

Next.js 16.2.3 frontend for the mikrom orchestration system.

**Stack:** Next.js 16.2.3 · React 19 · Tailwind CSS 4 · TypeScript · pnpm

> **Warning:** Next.js 16.2.3 has breaking changes relative to widely-known versions. Do not rely on assumptions about older Next.js APIs. Read `node_modules/next/dist/docs/` before writing framework-specific code.

## Development

```bash
pnpm install
pnpm dev      # development server (default port 3000)
```

## Available scripts

| Script | Description |
|---|---|
| `pnpm dev` | Start the development server with hot reload |
| `pnpm build` | Create a production build |
| `pnpm start` | Serve the production build |
| `pnpm lint` | Run ESLint |

## Configuration

The app communicates with `mikrom-api` (port 5001). Set the API base URL via environment variables in `.env.local`:

```
NEXT_PUBLIC_API_URL=http://localhost:5001
```

## Project structure

```
mikrom-app/
  app/          Next.js App Router pages and layouts
  public/       Static assets
  package.json
  tsconfig.json
```
