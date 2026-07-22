import { createContext, useContext } from "react";

import type { KomoConnectResponse } from "./global";

export type Mode = "interactive" | "trusted";

/** Gateway connection status, refreshed on a timer by `App`. */
export const ConnectionContext = createContext<KomoConnectResponse>({ connected: false });
export const useConnection = () => useContext(ConnectionContext);

/** App-wide state shared across the sidebar, chat, and settings modal:
 *  the active chat session and the turn trust mode. */
export interface AppState {
  session: string;
  setSession: (s: string) => void;
  mode: Mode;
  setMode: (m: Mode) => void;
}

export const AppContext = createContext<AppState>({
  session: "",
  setSession: () => {},
  mode: "interactive",
  setMode: () => {},
});
export const useApp = () => useContext(AppContext);
