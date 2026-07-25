// Class-based light/dark theme. The stylesheet flips every semantic token
// under `.dark`, so toggling the class on <html> re-themes the whole app.
// State lives in the store; this module owns only the DOM + storage side.

export type Theme = "light" | "dark";

const KEY = "komo.theme";

export function initialTheme(): Theme {
  try {
    const stored = localStorage.getItem(KEY);
    if (stored === "light" || stored === "dark") return stored;
  } catch {
    /* private mode / no storage */
  }
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function applyTheme(theme: Theme): void {
  document.documentElement.classList.toggle("dark", theme === "dark");
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    /* private mode / no storage */
  }
}
