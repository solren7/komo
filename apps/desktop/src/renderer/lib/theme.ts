// Class-based light/dark theme. theme.css flips every `--mc-*` token under
// `html.dark`, so toggling the class on <html> re-themes the whole app.

export type Theme = "light" | "dark";
const KEY = "komo.theme";

export function initialTheme(): Theme {
  try {
    const s = localStorage.getItem(KEY);
    if (s === "light" || s === "dark") return s;
  } catch {
    /* ignore */
  }
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function applyTheme(t: Theme): void {
  document.documentElement.classList.toggle("dark", t === "dark");
  try {
    localStorage.setItem(KEY, t);
  } catch {
    /* ignore */
  }
}
