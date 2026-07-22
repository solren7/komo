import { useQuery } from "@tanstack/react-query";

import { useApp, useConnection } from "./app-context";
import { apiField, fmtTs, newSessionId } from "./lib/ipc";
import type { SessionSummary } from "./types";

/** A short, human-readable label for a session id. */
function sessionLabel(id: string): string {
  const bare = id.replace(/^api:/, "");
  // gui-electron-<uuid> → last chunk of the uuid, so entries stay distinct.
  const m = bare.match(/gui-electron-.*?([0-9a-f]{4,})$/i);
  if (m) return `会话 ${m[1].slice(-6)}`;
  return bare.length > 22 ? `${bare.slice(0, 20)}…` : bare;
}

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
    <aside className="sidebar">
      <div className="sidebar-head">
        <span className="brand">komo</span>
        <span
          className={connected ? "dot online" : "dot offline"}
          title={connected ? "已连接" : "未连接"}
        />
      </div>

      <button className="new-session" onClick={() => setSession(newSessionId())}>
        ＋ 新建会话
      </button>

      <div className="session-list">
        {!known && (
          <button className="session-item active" onClick={() => {}}>
            <span className="session-title">新会话</span>
            <span className="session-meta">未开始</span>
          </button>
        )}
        {!connected ? (
          <div className="side-empty">未连接</div>
        ) : q.isPending ? (
          <div className="side-empty">加载中…</div>
        ) : sessions.length === 0 && known ? (
          <div className="side-empty">还没有会话</div>
        ) : (
          sessions.map((s) => (
            <button
              key={s.id}
              className={s.id === session ? "session-item active" : "session-item"}
              onClick={() => setSession(s.id)}
              title={s.id}
            >
              <span className="session-title">{sessionLabel(s.id)}</span>
              <span className="session-meta">
                {s.user_turns} 轮 · {fmtTs(s.created_at)}
              </span>
            </button>
          ))
        )}
      </div>

      <div className="sidebar-foot">
        <button className="foot-btn" onClick={onOpenSettings}>
          ⚙ 设置
        </button>
      </div>
    </aside>
  );
}
