import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function detectLanguage(name: string, gitUrl: string): string {
  const normalized = (name + " " + gitUrl).toLowerCase();
  
  if (normalized.includes("node") || normalized.includes("next") || normalized.includes("react") || normalized.includes("typescript") || normalized.includes("javascript") || normalized.includes("pnpm") || normalized.includes("npm")) return "Node.js";
  if (normalized.includes("rust") || normalized.includes("cargo") || normalized.includes("mikrom-api") || normalized.includes("mikrom-agent")) return "Rust";
  if (normalized.includes("go") || normalized.includes("golang") || normalized.includes("mikrom-router")) return "Go";
  if (normalized.includes("python") || normalized.includes("django") || normalized.includes("flask") || normalized.includes("fastapi")) return "Python";
  if (normalized.includes("php") || normalized.includes("laravel") || normalized.includes("symfony")) return "PHP";
  if (normalized.includes("ruby") || normalized.includes("rails")) return "Ruby";
  if (normalized.includes("java") || normalized.includes("spring") || normalized.includes("kotlin")) return "Java";
  if (normalized.includes("zig")) return "Zig";
  if (normalized.includes("deno")) return "Deno";
  
  return "Generic";
}
