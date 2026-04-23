# mikrom-app

The modern, real-time dashboard for the Mikrom PaaS. It allows users to manage their applications, monitor deployments, and view live logs from their Firecracker microVMs.

**Stack:** [Next.js 16](https://nextjs.org/) · [React 19](https://react.dev/) · [Tailwind CSS 4](https://tailwindcss.com/) · [Flowbite React](https://flowbite-react.com/)

**Port:** `3000`

## Key Features

- **Application Catalog**: Create and configure projects with custom Git URLs and ingress settings.
- **Deployment Management**: Launch new versions, stop instances, and track deployment history.
- **Live Logs**: Real-time console streaming using Server-Sent Events (SSE) from the Agente nodes.
- **Metric Visualization**: CPU, RAM, and Disk usage charts for every running microVM.
- **Responsive Design**: Fully optimized for desktop and mobile using Tailwind CSS 4.

## UI Conventions

Mikrom strictly follows the [Flowbite React](https://flowbite-react.com/) component library.
- **Strict Rule**: Do not create custom UI wrappers or abstractions in `components/ui`. Always import and use components directly from `flowbite-react`.
- **Styling**: Vanilla CSS is used for global styles, while Tailwind CSS 4 handles component-specific utility classes.

## Development

```bash
# Install dependencies
pnpm install

# Run the development server (http://localhost:3000)
pnpm dev

# Lint and check for Hydration/ESLint issues
pnpm lint
```

## Configuration

Set the target Mikrom API URL in `.env.local`:

```
NEXT_PUBLIC_API_URL=http://localhost:5001
```

## Internal Architecture

```
app/            # App Router pages (auth, deployments, settings)
components/     # Shared layout and modal components
lib/            # API clients, hooks, and utility functions
public/         # Static assets and favicons
```
