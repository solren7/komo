import type { ToolActivity } from "./turn-orchestrator";

/** Live feed of the turn's tool calls (from the stream's `event: tool` frames). */
export function ToolActivityStrip({ tools }: { tools: ToolActivity[] }) {
  if (tools.length === 0) return null;
  return (
    <div className="mx-4 mb-2 flex flex-col gap-1.5 rounded-xl border border-border bg-card px-3 py-2">
      <div className="text-xs tracking-wide text-muted-foreground uppercase">工具调用</div>
      {tools.map((tool) => (
        <div key={tool.seq} className="flex items-center gap-2 text-sm">
          <span className="w-4 text-center">{!tool.done ? "⏳" : tool.ok ? "✓" : "✗"}</span>
          <span className="font-mono font-semibold text-foreground">{tool.name}</span>
          <span className="flex-1 truncate text-muted-foreground">
            {tool.done ? (tool.summary ?? "") : tool.args}
          </span>
        </div>
      ))}
    </div>
  );
}
