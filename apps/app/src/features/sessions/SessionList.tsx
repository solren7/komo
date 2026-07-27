import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArchiveIcon,
  ArchiveRestoreIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  PencilIcon,
  PlusIcon,
  SettingsIcon,
  SlidersHorizontalIcon,
  Trash2Icon,
} from "lucide-react";

import { qk } from "@/shared/api/query-keys";
import { useConnection } from "@/shared/api/use-connection";
import { POLL } from "@/shared/config";
import { fmtTs } from "@/shared/lib/format";
import { cn } from "@/shared/lib/utils";
import { useAppStore, useNewWorkspace, useSession } from "@/shared/store";
import type { SessionSummary, WorkspaceInfo } from "@/shared/types";
import { Button } from "@/shared/ui/button";
import { IconButton } from "@/shared/ui/icon-button";
import { Input } from "@/shared/ui/input";
import { KomoLogo } from "@/shared/ui/komo-logo";
import { Popover, PopoverContent, PopoverTitle, PopoverTrigger } from "@/shared/ui/popover";
import { fetchSessions, renameSession, setSessionStatus } from "./api";
import { sessionLabel } from "./labels";
import { fetchWorkspaces } from "@/features/workspaces/api";
import { WorkspacePicker } from "@/features/workspaces/WorkspacePicker";

const ROW = "group flex w-full flex-col gap-0.5 rounded-lg px-2.5 py-2 transition-colors";
const DEFAULT_WORKSPACE = "__default__";

function workspaceLabel(id: string, workspaces: WorkspaceInfo[]): string {
  return workspaces.find((workspace) => workspace.id === id)?.name ??
    (id === DEFAULT_WORKSPACE ? "默认 workspace" : id);
}

export function SessionList({ onOpenSettings }: { onOpenSettings: () => void }) {
  const { connected } = useConnection();
  const session = useSession();
  const openSession = useAppStore((s) => s.openSession);
  const startNewSession = useAppStore((s) => s.startNewSession);
  const newWorkspace = useNewWorkspace();
  const setNewWorkspace = useAppStore((s) => s.setNewWorkspace);
  const pickedWorkspaces = useAppStore((s) => s.pickedWorkspaces);
  const qc = useQueryClient();
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [collapsed, setCollapsed] = useState(false);
  const [filter, setFilter] = useState<"active" | "archive" | "all">("active");

  const query = useQuery({
    queryKey: qk.sessions,
    queryFn: fetchSessions,
    refetchInterval: POLL.sessions,
    enabled: connected,
  });
  const sessions = query.data ?? [];
  const workspacesQuery = useQuery({
    queryKey: qk.workspaces,
    queryFn: fetchWorkspaces,
    enabled: connected,
  });
  const workspaces = [...(workspacesQuery.data ?? []), ...Object.values(pickedWorkspaces)].filter(
    (item, index, all) => all.findIndex((candidate) => candidate.id === item.id) === index,
  );

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

  const visibleSessions = sessions.filter((item) =>
    filter === "all"
      ? true
      : filter === "archive"
        ? item.status === "archive"
        : item.status !== "archive",
  );
  const groupedSessions = Array.from(
    visibleSessions.reduce((groups, item) => {
      const workspace = item.workspace ?? DEFAULT_WORKSPACE;
      const entries = groups.get(workspace) ?? [];
      entries.push(item);
      groups.set(workspace, entries);
      return groups;
    }, new Map<string, SessionSummary[]>()),
  ).map(([workspace, entries]) => ({
    workspace,
    label: workspaceLabel(workspace, workspaces),
    entries,
  }));

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
            onClick={() => openSession(item.id, item.workspace ?? DEFAULT_WORKSPACE)}
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
    <aside
      className={cn(
        "relative flex min-h-0 shrink-0 flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground transition-[width] duration-200",
        collapsed ? "w-14" : "w-[264px]",
      )}
    >
      <div
        className={cn(
          "flex h-12 shrink-0 items-center",
          collapsed ? "justify-center px-2" : "gap-2.5 px-4",
        )}
      >
        <KomoLogo className="size-7 shrink-0" />
        {!collapsed && <span className="font-bold tracking-wide">komo</span>}
        {!collapsed && <span className="flex-1" />}
        {!collapsed && (
          <span
            className={cn("size-2.5 rounded-full", connected ? "bg-emerald-500" : "bg-destructive")}
            title={connected ? "已连接" : "未连接"}
          />
        )}
        <Button
          variant="ghost"
          size="icon-xs"
          className={cn(
            collapsed &&
              "absolute -right-3 top-3 z-10 rounded-full border border-border bg-background shadow-sm",
          )}
          title={collapsed ? "展开侧边栏" : "折叠侧边栏"}
          onClick={() => setCollapsed((value) => !value)}
        >
          {collapsed ? <ChevronRightIcon /> : <ChevronLeftIcon />}
        </Button>
      </div>

      <div className={cn("flex flex-col gap-1 pb-2", collapsed ? "items-center px-2" : "px-3")}>
        {!collapsed && (
          <div className="flex min-w-0 px-2 py-1">
            <WorkspacePicker workspace={newWorkspace} onWorkspaceChange={setNewWorkspace} />
          </div>
        )}
        {/* New session only switches the active id — it does NOT add a row. The
            selected workspace is persisted with its first message. */}
        <Button
          className={collapsed ? "size-9 px-0" : "w-full"}
          onClick={startNewSession}
          title="新建会话"
        >
          <PlusIcon />
          {!collapsed && <span>新建会话</span>}
        </Button>
        <Popover>
          <PopoverTrigger
            aria-label="筛选会话"
            className={cn(
              "inline-flex h-9 items-center justify-center gap-2 rounded-md text-sm font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground",
              collapsed ? "w-9" : "w-full",
            )}
          >
            <SlidersHorizontalIcon className="size-4" />
            {!collapsed && <span>筛选会话</span>}
          </PopoverTrigger>
          <PopoverContent side="right" align="start" className="w-52 gap-1 p-2">
            <PopoverTitle className="px-2 py-1 text-xs text-muted-foreground">
              会话状态
            </PopoverTitle>
            {(
              [
                ["active", "进行中"],
                ["archive", "已归档"],
                ["all", "全部会话"],
              ] as const
            ).map(([value, label]) => (
              <Button
                key={value}
                variant={filter === value ? "secondary" : "ghost"}
                className="w-full justify-start"
                onClick={() => setFilter(value)}
              >
                <span
                  className={cn(
                    "size-2 rounded-full",
                    filter === value ? "bg-primary" : "bg-muted-foreground/30",
                  )}
                />
                {label}
              </Button>
            ))}
          </PopoverContent>
        </Popover>
      </div>

      {!collapsed && (
        <div className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto px-2 pb-2">
          {!connected ? (
            <div className="px-3 py-3 text-sm text-muted-foreground">未连接</div>
          ) : query.isPending ? (
            <div className="px-3 py-3 text-sm text-muted-foreground">加载中…</div>
          ) : visibleSessions.length === 0 ? (
            <div className="px-3 py-3 text-sm text-muted-foreground">没有符合条件的会话</div>
          ) : (
            groupedSessions.map((group) => (
              <section key={group.workspace} className="pt-2 first:pt-0">
                <h2 className="truncate px-3 pb-1 text-xs font-medium text-muted-foreground" title={group.label}>
                  {group.label}
                </h2>
                {group.entries.map(renderRow)}
              </section>
            ))
          )}
        </div>
      )}

      <div
        className={cn(
          "mt-auto border-t border-sidebar-border p-2",
          collapsed && "flex justify-center",
        )}
      >
        <Button
          variant="ghost"
          className={collapsed ? "size-9 px-0" : "w-full justify-start"}
          onClick={onOpenSettings}
          title="设置"
        >
          <SettingsIcon />
          {!collapsed && <span>设置</span>}
        </Button>
      </div>
    </aside>
  );
}
