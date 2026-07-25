import { LoaderIcon } from "lucide-react";

import { cn } from "@/shared/lib/utils";
import type { ToolActivity } from "./turn-orchestrator";

/** Live feed of the turn's tool calls (from the stream's `event: tool` frames).
 *
 *  Same restraint as a finished call in a message (`ToolCallView`): plain muted
 *  lines, no card, no heading. A spinner marks the call in flight; a finished
 *  one needs no mark, and only a failure takes a color. */
export function ToolActivityStrip({ tools }: { tools: ToolActivity[] }) {
  if (tools.length === 0) return null;
  return (
    <div className="mx-4 mb-1 flex flex-col gap-0.5 text-xs">
      {tools.map((tool) => {
        const failed = tool.done && !tool.ok;
        return (
          <div
            key={tool.seq}
            className={cn(
              "flex items-center gap-1.5",
              failed ? "text-destructive" : "text-muted-foreground",
            )}
          >
            <span className="grid size-3 shrink-0 place-items-center">
              {!tool.done && <LoaderIcon className="size-3 animate-spin" />}
            </span>
            <span className="shrink-0 font-mono">{tool.name}</span>
            <span className="truncate opacity-70">
              {tool.done ? (tool.summary ?? "") : tool.args}
            </span>
          </div>
        );
      })}
    </div>
  );
}
