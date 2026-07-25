import { LoaderIcon } from "lucide-react";

import { cn } from "@/shared/lib/utils";
import { ToolRoundHeader } from "./ToolCalls";
import { summarizeToolRound } from "./tool-summary";
import type { ToolActivity } from "./turn-orchestrator";

/** Live feed of the turn's tool calls (from the stream's `event: tool` frames).
 *
 *  Same shape as the finished round inside a message (`ToolCallGroup`): the whole
 *  round is one collapsible line, so nothing jumps when the turn lands and the
 *  message takes over. The line names the tool in flight — the one live fact
 *  worth reading without expanding — and a trailing spinner marks the call as
 *  running. Only a failure takes a color. */
export function ToolActivityStrip({ tools }: { tools: ToolActivity[] }) {
  if (tools.length === 0) return null;
  const running = tools.some((tool) => !tool.done);
  const summary = summarizeToolRound(
    tools.map((tool) => ({ name: tool.name, failed: tool.done && !tool.ok })),
  );

  return (
    <details className="group/round mx-4 mb-1 text-xs">
      <ToolRoundHeader
        lead={running ? "正在调用" : `${summary.count} 次工具调用`}
        summary={summary}
        trailing={running ? <LoaderIcon className="size-3 shrink-0 animate-spin" /> : undefined}
      />
      <div className="mt-0.5 ml-4 grid gap-0.5 text-muted-foreground">
        {tools.map((tool) => (
          <div
            key={tool.seq}
            className={cn(
              "flex items-center gap-1.5 py-0.5",
              tool.done && !tool.ok && "text-destructive",
            )}
          >
            <span className="shrink-0 font-mono">{tool.name}</span>
            <span className="truncate opacity-70">
              {tool.done ? (tool.summary ?? "") : tool.args}
            </span>
            {!tool.done && <LoaderIcon className="size-3 shrink-0 animate-spin" />}
          </div>
        ))}
      </div>
    </details>
  );
}
