import tailwindcss from "@tailwindcss/vite";
import { sveltekit } from "@sveltejs/kit/vite";
import { svelteTesting } from "@testing-library/svelte/vite";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [tailwindcss(), sveltekit(), svelteTesting()],
  server: {
    allowedHosts: ["mikrom.spluca.org"],
  },
  ssr: {
    noExternal: ["svelte-sonner"],
  },
  test: {
    environment: "happy-dom",
    globals: true,
    setupFiles: ["./tests/setup.ts"],
    include: ["src/**/*.{test,spec}.{ts,js}", "tests/unit/**/*.{test,spec}.{ts,js}"],
    coverage: {
      reporter: ["text", "lcov", "html"],
      include: ["src/lib/**/*.{ts,svelte}", "src/routes/**/*.{ts,svelte}"],
      exclude: ["src/routes/**/+page.server.ts", "src/routes/**/+layout.server.ts"],
    },
  },
});
