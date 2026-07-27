// Client-side state: the active immutable session/workspace pair, the workspace
// preselected for the next session, per-workspace trust, and theme.
// Server-side state lives in react-query, never here.
//
// Reopening the UI returns to the same conversation. A workspace is chosen
// only while creating a session, then travels with that session forever.

import { create } from "zustand";
import { persist } from "zustand/middleware";

import type { Mode, WorkspaceInfo } from "./types";
import { applyTheme, initialTheme, type Theme } from "./lib/theme";
import { newSessionId } from "./lib/session-id";
import { hostTag } from "./api/runtime";

export interface AppStore {
  session: string;
  workspace: string;
  newWorkspace: string;
  workspaceModes: Record<string, Mode>;
  /** Folders picked through the host's native dialog, keyed by workspace id.
   *  They are not in the gateway's catalog, so the client is what remembers
   *  them — hence persisted, like every other workspace-keyed slice here. */
  pickedWorkspaces: Record<string, WorkspaceInfo>;
  theme: Theme;
  openSession: (id: string, workspace: string) => void;
  startNewSession: () => void;
  setNewWorkspace: (id: string) => void;
  addWorkspace: (workspace: WorkspaceInfo) => void;
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
      newWorkspace: "__default__",
      workspaceModes: {},
      pickedWorkspaces: {},
      theme: initialTheme(),
      openSession: (session, workspace) => set({ session, workspace }),
      startNewSession: () =>
        set((s) => {
          const session = newSessionId(hostTag());
          return { session, workspace: s.newWorkspace };
        }),
      setNewWorkspace: (newWorkspace) => set({ newWorkspace }),
      // Picking a folder registers its display name locally. Selection is left
      // to the new-session control so an open conversation cannot be rebound.
      addWorkspace: (workspace) =>
        set((s) => ({
          pickedWorkspaces: { ...s.pickedWorkspaces, [workspace.id]: workspace },
        })),
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
        newWorkspace: s.newWorkspace,
        workspaceModes: s.workspaceModes,
        pickedWorkspaces: s.pickedWorkspaces,
        theme: s.theme,
      }),
    },
  ),
);

export const useSession = () => useAppStore((s) => s.session);
export const useWorkspace = () => useAppStore((s) => s.workspace);
export const useNewWorkspace = () => useAppStore((s) => s.newWorkspace);
export const useMode = (workspace?: string) =>
  useAppStore((s) => s.workspaceModes[workspace ?? s.workspace] ?? "interactive");
export const useTheme = () => useAppStore((s) => s.theme);
