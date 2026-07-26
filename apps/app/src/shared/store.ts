// Client-side state: active workspace/session, per-workspace trust, and theme.
// Server-side state lives in react-query, never here.
//
// Reopening the UI returns to the same conversation. Each workspace remembers
// both its last session and its trust mode, so changing projects cannot silently
// carry an auto-approve decision across the boundary.

import { create } from "zustand";
import { persist } from "zustand/middleware";

import type { Mode } from "./types";
import { applyTheme, initialTheme, type Theme } from "./lib/theme";
import { newSessionId } from "./lib/session-id";
import { hostTag } from "./api/runtime";

export interface AppStore {
  session: string;
  workspace: string;
  workspaceSessions: Record<string, string>;
  workspaceModes: Record<string, Mode>;
  theme: Theme;
  setSession: (id: string) => void;
  startNewSession: () => void;
  setWorkspace: (id: string) => void;
  setMode: (workspace: string, mode: Mode) => void;
  toggleTheme: () => void;
}

export const useAppStore = create<AppStore>()(
  persist(
    (set) => ({
      // Seeded by `installHost()` before the first render — the host tag isn't
      // known when this module is imported.
      session: "",
      workspace: "__default__",
      workspaceSessions: {},
      workspaceModes: {},
      theme: initialTheme(),
      setSession: (id) =>
        set((s) => ({
          session: id,
          workspaceSessions: { ...s.workspaceSessions, [s.workspace]: id },
        })),
      startNewSession: () =>
        set((s) => {
          const session = newSessionId(hostTag());
          return {
            session,
            workspaceSessions: { ...s.workspaceSessions, [s.workspace]: session },
          };
        }),
      setWorkspace: (workspace) =>
        set((s) => {
          const remembered = { ...s.workspaceSessions, [s.workspace]: s.session };
          const session = remembered[workspace] || newSessionId(hostTag());
          return {
            workspace,
            session,
            workspaceSessions: { ...remembered, [workspace]: session },
          };
        }),
      setMode: (workspace, mode) =>
        set((s) => ({ workspaceModes: { ...s.workspaceModes, [workspace]: mode } })),
      toggleTheme: () =>
        set((s) => {
          const theme: Theme = s.theme === "dark" ? "light" : "dark";
          applyTheme(theme);
          return { theme };
        }),
    }),
    {
      name: "komo.app",
      partialize: (s) => ({
        session: s.session,
        workspace: s.workspace,
        workspaceSessions: s.workspaceSessions,
        workspaceModes: s.workspaceModes,
        theme: s.theme,
      }),
    },
  ),
);

export const useSession = () => useAppStore((s) => s.session);
export const useWorkspace = () => useAppStore((s) => s.workspace);
export const useMode = (workspace?: string) =>
  useAppStore((s) => s.workspaceModes[workspace ?? s.workspace] ?? "interactive");
export const useTheme = () => useAppStore((s) => s.theme);
