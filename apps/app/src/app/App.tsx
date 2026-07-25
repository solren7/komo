import { useState } from "react";
import { MoonIcon, SunIcon } from "lucide-react";

import { useConnection } from "@/shared/api/use-connection";
import { useAppStore, useTheme } from "@/shared/store";
import { Button } from "@/shared/ui/button";
import { ChatView } from "@/features/chat/ChatView";
import { SessionList } from "@/features/sessions/SessionList";
import { SettingsModal } from "@/features/settings/SettingsModal";

export function App() {
  const connection = useConnection();
  const session = useAppStore((s) => s.session);
  const theme = useTheme();
  const toggleTheme = useAppStore((s) => s.toggleTheme);
  const [settingsOpen, setSettingsOpen] = useState(false);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <SessionList onOpenSettings={() => setSettingsOpen(true)} />

      <section className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-4">
          <span className="truncate text-sm font-medium text-muted-foreground">对话</span>
          <div className="flex-1" />
          <Button
            variant="ghost"
            size="icon"
            title={theme === "dark" ? "切换到亮色" : "切换到暗色"}
            onClick={toggleTheme}
          >
            {theme === "dark" ? <SunIcon /> : <MoonIcon />}
          </Button>
        </header>

        {!connection.connected && (
          <div className="shrink-0 border-b border-border bg-amber-500/10 px-4 py-1.5 text-sm text-amber-700 dark:text-amber-400">
            {connection.error ?? "正在连接 komo gateway…"}
          </div>
        )}

        {/* Keyed by session: switching sessions remounts the thread with its
            own history rather than trying to reconcile two transcripts. */}
        <ChatView key={session} />
      </section>

      {settingsOpen && <SettingsModal onClose={() => setSettingsOpen(false)} />}
    </div>
  );
}
