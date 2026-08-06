import { useEffect, useState } from "react";
import { FolderIcon, MoonIcon, PanelLeftIcon, SunIcon } from "lucide-react";

import { useConnection } from "@/shared/api/use-connection";
import { useAppStore, useTheme } from "@/shared/store";
import { Button } from "@/shared/ui/button";
import { ChatView } from "@/features/chat/ChatView";
import { SessionList } from "@/features/sessions/SessionList";
import { SettingsModal } from "@/features/settings/SettingsModal";

export function App() {
  const connection = useConnection();
  const session = useAppStore((s) => s.session);
  const workspace = useAppStore((s) => s.workspace);
  const theme = useTheme();
  const toggleTheme = useAppStore((s) => s.toggleTheme);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  // Visited sessions and the workspace each runs in, keyed by session id.
  //
  // Keyed by id alone — *not* id+workspace — because an unstarted session's
  // workspace is editable from the composer: a composite key would fork a second
  // ChatView on every change and leave the first one mounted. Refreshing the
  // stored workspace in place is safe precisely because it can only change before
  // the first turn, when nothing is running.
  const [openThreads, setOpenThreads] = useState<Record<string, string>>(() => ({
    [session]: workspace,
  }));

  useEffect(() => {
    setOpenThreads((current) =>
      current[session] === workspace ? current : { ...current, [session]: workspace },
    );
  }, [session, workspace]);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <SessionList
        mobileOpen={mobileNavOpen}
        onMobileOpenChange={setMobileNavOpen}
        onOpenSettings={() => setSettingsOpen(true)}
      />

      <section className="komo-workspace flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center gap-3 border-b border-border/80 bg-background/75 px-5">
          <Button
            variant="ghost"
            size="icon-sm"
            className="sm:hidden"
            title="打开会话列表"
            onClick={() => setMobileNavOpen(true)}
          >
            <PanelLeftIcon />
          </Button>
          <div className="flex min-w-0 items-center gap-2 text-sm">
            <span className="font-semibold tracking-tight">对话</span>
            <span className="h-3.5 w-px bg-border" aria-hidden="true" />
            <FolderIcon className="size-3.5 text-muted-foreground" aria-hidden="true" />
            <span className="truncate text-xs text-muted-foreground" title={workspace}>
              {workspace}
            </span>
          </div>
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
        {Object.entries(openThreads).map(([id, threadWorkspace]) => (
          <div
            key={id}
            className={id === session ? "komo-thread flex min-h-0 min-w-0 flex-1" : "hidden"}
          >
            <ChatView session={id} workspace={threadWorkspace} />
          </div>
        ))}
      </section>

      {settingsOpen && <SettingsModal onClose={() => setSettingsOpen(false)} />}
    </div>
  );
}
