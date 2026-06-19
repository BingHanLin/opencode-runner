// Theme persistence: read from localStorage, fall back to OS preference.
// Applied as a `data-theme` attribute on the <html> element. CSS variables
// in styles.css branch on `:root[data-theme="..."]`.

export type Theme = "dark" | "light";

const STORAGE_KEY = "runner.theme";

export function getInitialTheme(): Theme {
  if (typeof window === "undefined") return "dark";
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved === "dark" || saved === "light") return saved;
  } catch {
    /* localStorage may be blocked; ignore */
  }
  return window.matchMedia &&
    window.matchMedia("(prefers-color-scheme: light)").matches
    ? "light"
    : "dark";
}

export function applyTheme(theme: Theme) {
  document.documentElement.setAttribute("data-theme", theme);
}

export function saveTheme(theme: Theme) {
  try {
    window.localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    /* ignore */
  }
}
