# Mikrom today: a Rust PaaS that is now about operations as much as it is about code

Mikrom is a project that can no longer be described as just a deployment platform. It is still an open-source PaaS written in Rust, but its current state shows something more interesting: a fairly complete technical foundation, a usable product layer, and a clear effort to make validation, builds, and deployment part of the same operating model.

What stands out from the repository `README.md` is that Mikrom is no longer an abstract idea or an isolated prototype. It is a workspace with well-defined pieces:

- a control plane in `mikrom-api`;
- a modern dashboard in `mikrom-app`;
- a traffic router built on Pingora;
- network and internal name-resolution services in `mikrom-network` and `mikrom-dns`;
- an agent with eBPF integration;
- a builder, scheduler, and CLI that close the operational loop;
- and a local CI/CD runner built on Dagger that mirrors GitHub Actions validation.

That combination says a lot about the project’s maturity. Mikrom is not trying to solve only application deployment. It is covering the full platform lifecycle: user entry, build, scheduling, networking, observability, isolation, and repeatable validation.

## The platform already has shape

The current Mikrom architecture is clearly split into a control plane, a traffic plane, and internal infrastructure services.

In the control plane, the flow goes from the web UI or the CLI into `mikrom-api`, which orchestrates the rest of the system: builder, scheduler, and agent. In the traffic plane, `mikrom-router` handles external requests and routes them to the microVMs. Around all of that are the pieces that make the system actually operable: WireGuard mesh networking, internal DNS, microVM bootstrapping, and low-level support inside the agent.

This matters because it cleanly separates responsibilities. Mikrom does not mix control panel, routing, networking, and execution into a single monolith. The design points to a real platform, not a deployment demo.

## The frontend has matured too

Another clear sign of where the project stands is the dashboard. The repository now explicitly describes a frontend built with SvelteKit, Svelte 5, Vite, Tailwind CSS 4, shadcn-svelte, and bits-ui. That is not a superficial detail. It means the day-to-day experience of operating the platform is being treated as a first-class part of the product.

In other words, Mikrom does not live only in the terminal. It has a web surface designed for daily operations, making it possible to review apps, deployments, and resources without relying entirely on the CLI.

## The most recent work: CI as an internal product

If the `README.md` describes the overall picture, the recent Git log shows where the current engineering energy is going.

The latest commits show a clear pattern:

- the CI runner has been unified into a Rust-based Dagger system;
- GitHub Actions has stopped being a parallel implementation and now leans on that same logic;
- cache profiles have been split and tuned by pipeline type;
- builder validation and integration tests have been adjusted;
- and readiness checks for shared services have been hardened before running shared suites.

That is important because it changes how the project maintains itself. When a platform grows, the difference between “having CI” and “having a coherent validation system” is huge. Mikrom seems to have crossed that line: validation is no longer a collection of ad hoc scripts, but another layer of the product.

The practical upside is straightforward for anyone contributing to the project:

- less drift between local execution and GitHub Actions;
- better reproducibility;
- less noise from synchronization failures in supporting services;
- and a clearer path for validating large changes without depending on manual environments.

## The builder and tests are being tightened too

The recent changes in `mikrom-builder` point to one of the most common pain points in any deployment platform: coordination between events, queues, and timeouts.

The tests now rely on explicit NATS `flush()` calls and longer timeouts. That suggests concrete hardening work, not cosmetic cleanup. It also fits the broader picture: the project is stabilizing its internal guarantees so the build and notification pipeline does not depend on overly optimistic assumptions.

At the same time, the CI runner has started treating `mikrom-builder` as a special case inside the binary test group. That kind of adjustment usually appears when a project moves past the “everything works by coincidence” phase and into the “each component needs to be executed correctly” phase.

## What this says about the project right now

My read is that Mikrom is in a strong consolidation phase.

The work is no longer about adding disconnected components. It is about closing gaps between them:

- product and operations;
- dashboard and CLI;
- build and deployment;
- internal networking and external routing;
- local validation and remote CI.

That kind of evolution is often the most interesting signal in an infrastructure project. It is not the flashiest phase, but it is the one that turns a promising technical base into a platform that can sustain real growth.

## In short

Mikrom’s current state is that of a project moving from architecture to system. It has a solid Rust-first foundation, a modern web interface, a more serious traffic router, its own networking layer, and a low-level agent path. But above all, it is doing something many platforms postpone for too long: treating CI/CD as part of the product architecture.

That change does not just improve code quality. It improves the speed at which the project can evolve without breaking itself.
