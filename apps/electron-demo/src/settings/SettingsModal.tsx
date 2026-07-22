import { useState } from "react";

import { useApp, useConnection } from "../app-context";
import { MemoriesTab, RunsTab, StatusTab, TasksTab } from "./panels";

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
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal settings-modal" onClick={(e) => e.stopPropagation()}>
        <div className="settings-head">
          <div className="modal-title">设置</div>
          <button className="icon-btn" onClick={onClose} title="关闭">
            ✕
          </button>
        </div>

        <div className="dash-tabs">
          {TABS.map(([t, label]) => (
            <button
              key={t}
              className={tab === t ? "dash-tab active" : "dash-tab"}
              onClick={() => setTab(t)}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="settings-body">
          {tab === "general" ? (
            <GeneralTab />
          ) : !connected ? (
            <div className="empty">未连接到 gateway。</div>
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

function GeneralTab() {
  const { connected } = useConnection();
  const { mode, setMode } = useApp();
  return (
    <div className="panel">
      <label className="setting-row">
        <div className="setting-text">
          <div className="setting-name">信任模式（自动批准）</div>
          <div className="setting-desc">
            开启后副作用工具自动批准（等同 komo chat）；关闭则弹出审批。
          </div>
        </div>
        <input
          type="checkbox"
          checked={mode === "trusted"}
          onChange={(e) => setMode(e.target.checked ? "trusted" : "interactive")}
        />
      </label>

      <div className="setting-row">
        <div className="setting-text">
          <div className="setting-name">连接状态</div>
          <div className="setting-desc">komo gateway 的实时连接。</div>
        </div>
        <span className={connected ? "chip ok" : "chip warn"}>
          {connected ? "已连接" : "未连接"}
        </span>
      </div>

      {connected && <StatusTab />}
    </div>
  );
}
