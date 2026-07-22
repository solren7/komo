import { useEffect, useState } from "react";

import type { KomoConnectResponse } from "./global";
import { AppContext, ConnectionContext, type Mode } from "./app-context";
import { ChatView } from "./chat/ChatView";
import { Sidebar } from "./Sidebar";
import { SettingsModal } from "./settings/SettingsModal";
import { newSessionId } from "./lib/ipc";
import { applyTheme, initialTheme, type Theme } from "./lib/theme";
import { Button } from "@/components/ui/button";

export function App() {
  const [conn, setConn] = useState<KomoConnectResponse>({ connected: false });
  const [session, setSession] = useState<string>(() => newSessionId());
  const [mode, setMode] = useState<Mode>("interactive");
  const [theme, setTheme] = useState<Theme>(() => initialTheme());
  const [settingsOpen, setSettingsOpen] = useState(false);

  const toggleTheme = () => {
    setTheme((t) => {
      const next: Theme = t === "dark" ? "light" : "dark";
      applyTheme(next);
      return next;
    });
  };

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
      <AppContext.Provider value={{ session, setSession, mode, setMode, theme, toggleTheme }}>
        <div className="app w-screen h-screen flex overflow-hidden bg-(--mc-bg) text-(--mc-fg)">
          <Sidebar onOpenSettings={() => setSettingsOpen(true)} />

          <section className="flex-1 flex flex-col min-w-0 bg-(--mc-bg)">
            <header className="h-12 shrink-0 px-4 flex items-center gap-2 border-b border-(--mc-border)">
              <span className="text-sm font-medium text-(--mc-fg-muted) truncate">对话</span>
              <div className="flex-1" />
              <Button
                variant="ghost"
                size="icon"
                className="text-(--mc-fg-muted) hover:text-(--mc-fg)"
                title={theme === "dark" ? "切换到亮色" : "切换到暗色"}
                onClick={toggleTheme}
              >
                {theme === "dark" ? <SunIcon /> : <MoonIcon />}
              </Button>
            </header>

            {!conn.connected && (
              <div className="shrink-0 px-4 py-1.5 text-[13px] border-b border-(--mc-border) text-(--mc-warn) bg-[color-mix(in_srgb,var(--mc-warn)_12%,transparent)]">
                {conn.error ?? "正在连接 komo gateway…"}
              </div>
            )}

            <ChatView key={session} />
          </section>

          {settingsOpen && <SettingsModal onClose={() => setSettingsOpen(false)} />}
        </div>
      </AppContext.Provider>
    </ConnectionContext.Provider>
  );
}

function SunIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
    </svg>
  );
}

function MoonIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
    </svg>
  );
}
