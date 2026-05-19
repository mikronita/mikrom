import { writable } from "svelte/store";

const THEME_KEY = "mikrom_theme";
export type Theme = "light" | "dark";
export const themeStore = writable<Theme>("light");

function applyTheme(theme: Theme) {
  if (typeof document === "undefined") return;
  document.documentElement.classList.toggle("dark", theme === "dark");
  document.documentElement.classList.toggle("light-theme", theme === "light");
  document.documentElement.style.colorScheme = theme;
  window.dispatchEvent(new CustomEvent("mikrom-theme-change", { detail: theme }));
}

export function initTheme() {
  if (typeof window === "undefined") return;
  const stored = localStorage.getItem(THEME_KEY);
  const prefersDark = window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
  const next = stored === "light" || stored === "dark" ? stored : prefersDark ? "dark" : "light";
  applyTheme(next);
  themeStore.set(next);
  localStorage.setItem(THEME_KEY, next);
}

export function getTheme() {
  if (typeof window === "undefined") return "light";
  if (document.documentElement.classList.contains("dark")) return "dark";
  return localStorage.getItem(THEME_KEY) === "dark" ? "dark" : "light";
}

export function setTheme(theme: Theme) {
  if (typeof document === "undefined") return;
  applyTheme(theme);
  themeStore.set(theme);
  localStorage.setItem(THEME_KEY, theme);
}

export function toggleTheme() {
  const next = document.documentElement.classList.contains("dark") ? "light" : "dark";
  setTheme(next);
}
