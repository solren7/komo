import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArchiveIcon,
  ArchiveRestoreIcon,
  PencilIcon,
  PlusIcon,
  SettingsIcon,
  Trash2Icon,
} from "lucide-react";

import { qk } from "@/shared/api/query-keys";
import { useConnection } from "@/shared/api/use-connection";
import { POLL } from "@/shared/config";
import { fmtTs } from "@/shared/lib/format";
import { cn } from "@/shared/lib/utils";
import { useAppStore, useSession } from "@/shared/store";
import type { SessionSummary } from "@/shared/types";
import { Button } from "@/shared/ui/button";
import { IconButton } from "@/shared/ui/icon-button";
import { Input } from "@/shared/ui/input";
import { KomoLogo } from "@/shared/ui/komo-logo";
import { fetchSessions, renameSession, setSessionStatus } from "./api";
import { sessionLabel } from "./labels";

const ROW = "group flex w-full flex-col gap-0.5 rounded-lg px-2.5 py-2 transition-colors";

export function SessionList({ onOpenSettings }: { onOpenSettings: () => void }) {
  const { connected } = useConnection();
  const session = useSession();
  const setSession = useAppStore((s) => s.setSession);
  const startNewSession = useAppStore((s) => s.startNewSession);
  const qc = useQueryClient();
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [showArchived, setShowArchived] = useState(false);

  const query = useQuery({
    queryKey: qk.sessions,
    queryFn: fetchSessions,
    refetchInterval: POLL.sessions,
    enabled: connected,
  });
  const sessions = query.data ?? [];

  const invalidate = () => void qc.invalidateQueries({ queryKey: qk.sessions });

  const rename = useMutation({
    mutationFn: ({ id, title }: { id: string; title: string }) => renameSession(id, title),
    onSettled: invalidate,
  });

  const restatus = useMutation({
    mutationFn: ({ id, status }: { id: string; status: string }) => setSessionStatus(id, status),
    onSuccess: (_data, { id, status }) => {
      // Leaving the open session (deleted/archived) → drop into a fresh one.
      if (id === session && status !== "active") startNewSession();
    },
    onSettled: invalidate,
  });

  const commitRename = (id: string) => {
    const title = draft.trim();
    setEditingId(null);
    rename.mutate({ id, title });
  };

  const remove = (id: string) => {
    if (!window.confirm("删除该会话？（软删除，从列表移除）")) return;
    restatus.mutate({ id, status: "deleted" });
  };

  const active = sessions.filter((s) => s.status !== "archive");
  const archived = sessions.filter((s) => s.status === "archive");

  const renderRow = (item: SessionSummary) => {
    const isOpen = item.id === session;
    const isArchived = item.status === "archive";
    const label = item.title?.trim() ? item.title : sessionLabel(item.id);
    const tint = isOpen
      ? "bg-primary/10 ring-1 ring-primary/20"
      : "hover:bg-sidebar-accent hover:text-sidebar-accent-foreground";

    if (editingId === item.id) {
      return (
        <div key={item.id} className={cn(ROW, tint)}>
          <Input
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitRename(item.id);
              else if (e.key === "Escape") setEditingId(null);
            }}
            onBlur={() => commitRename(item.id)}
            className="h-7 text-sm"
          />
        </div>
      );
    }

    return (
      <div key={item.id} className={cn(ROW, tint)}>
        <div className="flex items-center gap-1">
          <button
            type="button"
            className="min-w-0 flex-1 text-left"
            onClick={() => setSession(item.id)}
            title={item.id}
          >
            <span className="block truncate text-sm">{label}</span>
            <span className="text-xs text-muted-foreground">
              {item.user_turns} 轮 · {fmtTs(item.created_at)}
            </span>
          </button>
          <div className="flex shrink-0 gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
            <IconButton
              title="重命名"
              onClick={() => {
                setDraft(item.title ?? "");
                setEditingId(item.id);
              }}
            >
              <PencilIcon className="size-3.5" />
            </IconButton>
            {isArchived ? (
              <IconButton
                title="取消归档"
                onClick={() => restatus.mutate({ id: item.id, status: "active" })}
              >
                <ArchiveRestoreIcon className="size-3.5" />
              </IconButton>
            ) : (
              <IconButton
                title="归档"
                onClick={() => restatus.mutate({ id: item.id, status: "archive" })}
              >
                <ArchiveIcon className="size-3.5" />
              </IconButton>
            )}
            <IconButton title="删除" danger onClick={() => remove(item.id)}>
              <Trash2Icon className="size-3.5" />
            </IconButton>
          </div>
        </div>
      </div>
    );
  };

  return (
    <aside className="flex w-[264px] shrink-0 flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground min-h-0">
      <div className="flex h-12 shrink-0 items-center gap-2.5 px-4">
        <KomoLogo className="size-7" />
        <span className="font-bold tracking-wide">komo</span>
        <span className="flex-1" />
        <span
          className={cn("size-2.5 rounded-full", connected ? "bg-emerald-500" : "bg-destructive")}
          title={connected ? "已连接" : "未连接"}
        />
      </div>

      <div className="px-3 pb-2">
        {/* New session only switches the active id — it does NOT add a row. The
            session appears in the list once the first message creates it. */}
        <Button className="w-full" onClick={startNewSession}>
          <PlusIcon />
          <span>新建会话</span>
        </Button>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto px-2 pb-2">
        {!connected ? (
          <div className="px-3 py-3 text-sm text-muted-foreground">未连接</div>
        ) : query.isPending ? (
          <div className="px-3 py-3 text-sm text-muted-foreground">加载中…</div>
        ) : sessions.length === 0 ? (
          <div className="px-3 py-3 text-sm text-muted-foreground">还没有会话</div>
        ) : (
          <>
            {active.map(renderRow)}
            {archived.length > 0 && (
              <div className="mt-1 flex flex-col gap-0.5">
                <button
                  type="button"
                  className="px-2.5 py-1.5 text-left text-xs text-muted-foreground hover:text-foreground"
                  onClick={() => setShowArchived((v) => !v)}
                >
                  {showArchived ? "▾" : "▸"} 已归档 ({archived.length})
                </button>
                {showArchived && archived.map(renderRow)}
              </div>
            )}
          </>
        )}
      </div>

      <div className="border-t border-sidebar-border p-2">
        <Button variant="ghost" className="w-full justify-start" onClick={onOpenSettings}>
          <SettingsIcon />
          <span>设置</span>
        </Button>
      </div>
    </aside>
  );
}
