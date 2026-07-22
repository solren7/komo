import { useState } from "react";

import { useApp, useConnection } from "../app-context";
import { MemoriesTab, RunsTab, StatusTab, TasksTab } from "./panels";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";

type Tab = "general" | "tasks" | "memories" | "runs";
const TABS: [Tab, string][] = [
  ["general", "常规"],
  ["tasks", "任务"],
  ["memories", "记忆"],
  ["runs", "运行"],
];

export function SettingsModal({ onClose }: { onClose: () => void }) {
  const [tab, setTab] = useState<Tab>("general");
  const { connected } = useConnection();

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/45 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="w-[min(620px,92vw)] max-h-[82vh] flex flex-col overflow-hidden rounded-2xl bg-(--mc-bg-elev) border border-(--mc-border-strong) shadow-(--mc-shadow-card)"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 pt-4 pb-2.5">
          <div className="font-bold text-(--mc-fg)">设置</div>
          <Button
            variant="ghost"
            size="icon"
            className="size-7 text-(--mc-fg-muted) hover:text-(--mc-fg)"
            onClick={onClose}
            title="关闭"
          >
            ✕
          </Button>
        </div>

        <div className="flex gap-1 px-5 border-b border-(--mc-border)">
          {TABS.map(([t, label]) => (
            <button
              key={t}
              className={`px-3 py-2 -mb-px border-b-2 text-[13px] cursor-pointer transition-colors ${
                tab === t
                  ? "border-(--mc-accent) text-(--mc-fg)"
                  : "border-transparent text-(--mc-fg-muted) hover:text-(--mc-fg)"
              }`}
              onClick={() => setTab(t)}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="flex-1 overflow-y-auto min-h-0 px-5 py-4">
          {tab === "general" ? (
            <GeneralTab />
          ) : !connected ? (
            <Empty>未连接到 gateway。</Empty>
          ) : tab === "tasks" ? (
            <TasksTab />
          ) : tab === "memories" ? (
            <MemoriesTab />
          ) : (
            <RunsTab />
          )}
        </div>
      </div>
    </div>
  );
}

export function Empty({ children }: { children: React.ReactNode }) {
  return <div className="flex items-center justify-center py-8 text-(--mc-fg-faint)">{children}</div>;
}

function GeneralTab() {
  const { connected } = useConnection();
  const { mode, setMode } = useApp();
  return (
    <div className="flex flex-col">
      <label className="flex items-center justify-between gap-4 py-3 border-b border-(--mc-border) cursor-pointer">
        <div>
          <div className="text-sm text-(--mc-fg)">信任模式（自动批准）</div>
          <div className="text-xs text-(--mc-fg-muted) mt-0.5">
            开启后副作用工具自动批准（等同 komo chat）；关闭则弹出审批。
          </div>
        </div>
        <Switch
          checked={mode === "trusted"}
          onCheckedChange={(v) => setMode(v ? "trusted" : "interactive")}
        />
      </label>

      <div className="flex items-center justify-between gap-4 py-3 border-b border-(--mc-border)">
        <div>
          <div className="text-sm text-(--mc-fg)">连接状态</div>
          <div className="text-xs text-(--mc-fg-muted) mt-0.5">komo gateway 的实时连接。</div>
        </div>
        <Badge variant={connected ? "ok" : "warn"} className="rounded-full px-2 py-1">
          {connected ? "已连接" : "未连接"}
        </Badge>
      </div>

      {connected && (
        <div className="pt-4">
          <StatusTab />
        </div>
      )}
    </div>
  );
}
