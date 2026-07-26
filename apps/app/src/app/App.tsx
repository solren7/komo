import { useEffect, useState } from "react";
import { MoonIcon, SunIcon } from "lucide-react";

import { useConnection } from "@/shared/api/use-connection";
import { useAppStore, useTheme } from "@/shared/store";
import { Button } from "@/shared/ui/button";
import { ChatView } from "@/features/chat/ChatView";
import { SessionList } from "@/features/sessions/SessionList";
import { SettingsModal } from "@/features/settings/SettingsModal";
import { WorkspacePicker } from "@/features/workspaces/WorkspacePicker";

export function App() {
  const connection = useConnection();
  const session = useAppStore((s) => s.session);
  const workspace = useAppStore((s) => s.workspace);
  const theme = useTheme();
  const toggleTheme = useAppStore((s) => s.toggleTheme);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [openThreads, setOpenThreads] = useState<Record<string, { session: string; workspace: string }>>(
    () => ({ [`${workspace}\u0000${session}`]: { session, workspace } }),
  );
  const activeThread = `${workspace}\u0000${session}`;

  useEffect(() => {
    setOpenThreads((current) =>
      current[activeThread]
        ? current
        : { ...current, [activeThread]: { session, workspace } },
    );
  }, [activeThread, session, workspace]);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <SessionList onOpenSettings={() => setSettingsOpen(true)} />

      <section className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-4">
          <WorkspacePicker />
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
            {connection.error ?? "Connecting…"}
          </div>
        )}

        {/* Keep visited runtimes mounted. assistant-ui aborts a request when its
            runtime unmounts, so navigation must only hide a running thread. */}
        {Object.entries(openThreads).map(([key, thread]) => (
          <div key={key} className={key === activeThread ? "flex min-h-0 flex-1" : "hidden"}>
            <ChatView session={thread.session} workspace={thread.workspace} />
          </div>
        ))}
      </section>

      {settingsOpen && <SettingsModal onClose={() => setSettingsOpen(false)} />}
    </div>
  );
}
