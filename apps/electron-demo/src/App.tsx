import { useEffect, useState } from "react";

import type { KomoConnectResponse } from "./global";
import { AppContext, ConnectionContext, type Mode } from "./app-context";
import { ChatView } from "./chat/ChatView";
import { Sidebar } from "./Sidebar";
import { SettingsModal } from "./settings/SettingsModal";
import { newSessionId } from "./lib/ipc";

export function App() {
  const [conn, setConn] = useState<KomoConnectResponse>({ connected: false });
  const [session, setSession] = useState<string>(() => newSessionId());
  const [mode, setMode] = useState<Mode>("interactive");
  const [settingsOpen, setSettingsOpen] = useState(false);

  // Connection lifecycle: probe on mount, then every 3s — attach when the
  // gateway starts, show offline when it stops.
  useEffect(() => {
    let alive = true;
    const tick = async () => {
      const r = await window.komo.connect();
      if (alive) setConn(r);
    };
    void tick();
    const id = setInterval(tick, 3000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  return (
    <ConnectionContext.Provider value={conn}>
      <AppContext.Provider value={{ session, setSession, mode, setMode }}>
        <div className="app">
          <Sidebar onOpenSettings={() => setSettingsOpen(true)} />

          <div className="main">
            {!conn.connected && (
              <div className="banner">{conn.error ?? "正在连接 komo gateway…"}</div>
            )}
            <ChatView key={session} />
          </div>

          {settingsOpen && <SettingsModal onClose={() => setSettingsOpen(false)} />}
        </div>
      </AppContext.Provider>
    </ConnectionContext.Provider>
  );
}
