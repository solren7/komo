import { createContext, useContext } from "react";

import type { KomoConnectResponse } from "./global";
import type { Theme } from "./lib/theme";

export type Mode = "interactive" | "trusted";

/** Gateway connection status, refreshed on a timer by `App`. */
export const ConnectionContext = createContext<KomoConnectResponse>({ connected: false });
export const useConnection = () => useContext(ConnectionContext);

/** App-wide state shared across the sidebar, chat, and settings modal:
 *  the active chat session, the turn trust mode, and the light/dark theme. */
export interface AppState {
  session: string;
  setSession: (s: string) => void;
  mode: Mode;
  setMode: (m: Mode) => void;
  theme: Theme;
  toggleTheme: () => void;
}

export const AppContext = createContext<AppState>({
  session: "",
  setSession: () => {},
  mode: "interactive",
  setMode: () => {},
  theme: "dark",
  toggleTheme: () => {},
});
export const useApp = () => useContext(AppContext);
