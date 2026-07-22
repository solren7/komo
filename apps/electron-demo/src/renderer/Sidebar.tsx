import { useQuery } from "@tanstack/react-query";

import { useApp, useConnection } from "./app-context";
import { apiField, fmtTs, newSessionId } from "./lib/ipc";
import type { SessionSummary } from "./types";
import { Button } from "@/components/ui/button";

/** A short, human-readable label for a session id. */
function sessionLabel(id: string): string {
  const bare = id.replace(/^api:/, "");
  // gui-electron-<uuid> → last chunk of the uuid, so entries stay distinct.
  const m = bare.match(/gui-electron-.*?([0-9a-f]{4,})$/i);
  if (m) return `会话 ${m[1].slice(-6)}`;
  return bare.length > 22 ? `${bare.slice(0, 20)}…` : bare;
}

const itemBase =
  "w-full text-left flex flex-col gap-0.5 px-2.5 py-2 rounded-[10px] cursor-pointer transition-colors";

export function Sidebar({ onOpenSettings }: { onOpenSettings: () => void }) {
  const { connected } = useConnection();
  const { session, setSession } = useApp();

  const q = useQuery({
    queryKey: ["sessions"],
    queryFn: () => apiField<SessionSummary[]>("/api/sessions", "sessions"),
    refetchInterval: 6000,
    enabled: connected,
  });

  const sessions = q.data ?? [];
  // The active session may be brand new (no server record yet) — surface it at
  // the top so the current conversation is always visible and highlighted.
  const known = sessions.some((s) => s.id === session);

  return (
    <aside className="w-[264px] shrink-0 flex flex-col min-h-0 border-r border-(--mc-border) bg-(--mc-surface) backdrop-blur-xl">
      <div className="h-12 shrink-0 px-4 flex items-center gap-2.5">
        <span
          className="w-6 h-6 rounded-[7px] shrink-0 shadow-(--mc-shadow-glow)"
          style={{ background: "var(--mc-accent-grad)" }}
        />
        <span className="font-bold tracking-wide text-(--mc-fg)">komo</span>
        <span className="flex-1" />
        <span
          className={`w-2.5 h-2.5 rounded-full ${connected ? "bg-(--mc-ok)" : "bg-(--mc-danger)"}`}
          title={connected ? "已连接" : "未连接"}
        />
      </div>

      <div className="px-3 pb-2">
        <Button variant="gradient" className="w-full" onClick={() => setSession(newSessionId())}>
          <PlusIcon />
          <span>新建会话</span>
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto min-h-0 px-2 pb-2 flex flex-col gap-0.5">
        {!known && (
          <div className={`${itemBase} bg-(--mc-accent-soft) ring-1 ring-(--mc-accent-ring)`}>
            <span className="text-[13px] text-(--mc-fg) truncate">新会话</span>
            <span className="text-[11px] text-(--mc-fg-faint)">未开始</span>
          </div>
        )}
        {!connected ? (
          <div className="px-3 py-3 text-[13px] text-(--mc-fg-faint)">未连接</div>
        ) : q.isPending ? (
          <div className="px-3 py-3 text-[13px] text-(--mc-fg-faint)">加载中…</div>
        ) : sessions.length === 0 && known ? (
          <div className="px-3 py-3 text-[13px] text-(--mc-fg-faint)">还没有会话</div>
        ) : (
          sessions.map((s) => {
            const active = s.id === session;
            return (
              <button
                key={s.id}
                className={`${itemBase} ${
                  active
                    ? "bg-(--mc-accent-soft) ring-1 ring-(--mc-accent-ring) text-(--mc-fg)"
                    : "text-(--mc-fg) hover:bg-(--mc-surface-2)"
                }`}
                onClick={() => setSession(s.id)}
                title={s.id}
              >
                <span className="text-[13px] truncate">{sessionLabel(s.id)}</span>
                <span className="text-[11px] text-(--mc-fg-faint)">
                  {s.user_turns} 轮 · {fmtTs(s.created_at)}
                </span>
              </button>
            );
          })
        )}
      </div>

      <div className="border-t border-(--mc-border) p-2">
        <Button
          variant="ghost"
          className="w-full justify-start text-(--mc-fg-muted) hover:text-(--mc-fg)"
          onClick={onOpenSettings}
        >
          <GearIcon />
          <span>设置</span>
        </Button>
      </div>
    </aside>
  );
}

function PlusIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

function GearIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}
