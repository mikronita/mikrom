# mikrom-app

The modern, real-time dashboard for the Mikrom PaaS. It allows users to manage their applications, monitor deployments, and view live logs from their Firecracker microVMs.

**Stack:** [Next.js 16](https://nextjs.org/) · [React 19](https://react.dev/) · [Tailwind CSS 4](https://tailwindcss.com/) · [shadcn/ui](https://ui.shadcn.com/)

**Port:** `3000`

## Key Features

- **Application Catalog**: Create and configure projects with custom Git URLs and ingress settings.
- **Deployment Management**: Launch new versions, stop instances, and track deployment history.
- **Secrets Management**: Securely manage environment variables and secrets via the UI.
- **GitHub Integration**: Link repositories and configure automatic deployments.
- **Health Configuration**: Define custom liveness and readiness probes for your applications.
- **Live Logs**: Real-time console streaming using Server-Sent Events (SSE) from the Agente nodes.
- **Metric Visualization**: CPU, RAM, and Disk usage charts for every running microVM.
- **Responsive Design**: Fully optimized for desktop and mobile using Tailwind CSS 4.

## UI Conventions

Mikrom uses [shadcn/ui](https://ui.shadcn.com/) for its component library.
- **Component Architecture**: Primitive components are located in `components/ui`.
- **Composition Rules**: Follow strict shadcn composition: use `FieldGroup` + `Field` for forms and `InputGroup` for decorated inputs.
- **Theming**: Supports Dark/Light mode using `next-themes` and CSS variables.
- **Styling**: Tailwind CSS 4 handles component-specific utility classes and variable-based theming.

## Development

```bash
# Install dependencies
pnpm install

# Run the development server (http://localhost:3000)
pnpm dev

# Lint and check for issues
pnpm lint

# Build for production
pnpm build
```
