// Client-side state: the active session, the turn trust mode, the theme.
// Server-side state lives in react-query, never here.
//
// `mode` and `theme` persist (a restart shouldn't silently drop back to
// interactive/light); `session` deliberately does NOT — every launch starts a
// fresh session, matching `komo chat` and the TUI. The sidebar is how you get
// back into an old one.

import { create } from "zustand";
import { persist } from "zustand/middleware";

import type { Mode } from "./types";
import { applyTheme, initialTheme, type Theme } from "./lib/theme";
import { newSessionId } from "./lib/session-id";
import { hostTag } from "./api/runtime";

export interface AppStore {
  session: string;
  mode: Mode;
  theme: Theme;
  setSession: (id: string) => void;
  startNewSession: () => void;
  setMode: (mode: Mode) => void;
  toggleTheme: () => void;
}

export const useAppStore = create<AppStore>()(
  persist(
    (set) => ({
      // Seeded by `installHost()` before the first render — the host tag isn't
      // known when this module is imported.
      session: "",
      mode: "interactive",
      theme: initialTheme(),
      setSession: (id) => set({ session: id }),
      startNewSession: () => set({ session: newSessionId(hostTag()) }),
      setMode: (mode) => set({ mode }),
      toggleTheme: () =>
        set((s) => {
          const theme: Theme = s.theme === "dark" ? "light" : "dark";
          applyTheme(theme);
          return { theme };
        }),
    }),
    {
      name: "komo.app",
      // Only client preferences survive a reload.
      partialize: (s) => ({ mode: s.mode, theme: s.theme }),
    },
  ),
);

export const useSession = () => useAppStore((s) => s.session);
export const useMode = () => useAppStore((s) => s.mode);
export const useTheme = () => useAppStore((s) => s.theme);
