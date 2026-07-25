import { type PropsWithChildren, type ReactNode, useMemo } from "react";
import { useAuiState, type ToolCallMessagePartProps } from "@assistant-ui/react";
import { ChevronRightIcon } from "lucide-react";

import { cn } from "@/shared/lib/utils";
import {
  summarizeToolRound,
  toolTitle,
  type ToolRoundCall,
  type ToolRoundSummary,
} from "./tool-summary";

/** The one line a collapsed round shows, shared by a finished round in a message
 *  and the live strip so the two read the same. `lead` says what the round is
 *  (a count, or what it is doing); `trailing` is the strip's spinner. */
export function ToolRoundHeader({
  lead,
  summary,
  trailing,
}: {
  lead: string;
  summary: ToolRoundSummary;
  trailing?: ReactNode;
}) {
  return (
    <summary
      className={cn(
        "flex list-none items-center gap-1.5 py-0.5 text-xs transition-colors [&::-webkit-details-marker]:hidden",
        summary.failed > 0 ? "text-destructive" : "text-muted-foreground hover:text-foreground",
      )}
    >
      <ChevronRightIcon className="size-3 shrink-0 transition-transform group-open/round:rotate-90" />
      <span className="shrink-0">{lead}</span>
      {summary.failed > 0 && <span className="shrink-0">· {summary.failed} 个失败</span>}
      <span className="truncate font-mono opacity-70">{summary.names}</span>
      {trailing}
    </summary>
  );
}

/** A turn's tool calls, collapsed to one line.
 *
 *  A turn can spend a dozen calls before it answers and none of them is the
 *  answer, so the whole round gets one line — `3 次工具调用  shell · time` —
 *  which expands to the list of calls, each of which expands again to its own
 *  arguments and result. Deliberately chrome-less at every level: no card, no
 *  fill, the disclosure arrow is the only affordance.
 *
 *  A single call skips the wrapper: it is already one line, and a group row
 *  around it would only cost a click. */
export function ToolCallGroup({
  indices,
  children,
}: PropsWithChildren<{ indices: readonly number[] }>) {
  if (indices.length < 2) return <>{children}</>;
  return <ToolRoundLine indices={indices}>{children}</ToolRoundLine>;
}

function ToolRoundLine({ indices, children }: PropsWithChildren<{ indices: readonly number[] }>) {
  // The group node carries indices, not parts — read the calls it points at so
  // the collapsed line can name the tools that ran.
  const parts = useAuiState((state) => state.message.parts);
  const summary = useMemo(() => {
    const calls: ToolRoundCall[] = indices.map((index) => {
      const part = parts[index];
      const call = part?.type === "tool-call" ? part : undefined;
      return { name: call?.toolName ?? "tool", failed: call?.isError === true };
    });
    return summarizeToolRound(calls);
  }, [indices, parts]);

  return (
    <details className="group/round my-0.5">
      <ToolRoundHeader lead={`${summary.count} 次工具调用`} summary={summary} />
      <div className="mt-0.5 mb-1 ml-4 grid">{children}</div>
    </details>
  );
}

/** One tool call: a quiet line that expands in place to the raw arguments and
 *  the (truncated) result. Success gets no color; a failure turns the line
 *  `text-destructive`. */
export function ToolCallView({
  toolName,
  args,
  argsText,
  result,
  isError,
}: ToolCallMessagePartProps) {
  const action = toolName === "skill" && typeof args?.action === "string" ? args.action : null;
  const detail = argsText || (args ? JSON.stringify(args, null, 2) : "");
  const output =
    typeof result === "string" ? result : result == null ? "" : JSON.stringify(result, null, 2);

  return (
    <details className="group/tool my-0.5">
      <summary
        className={cn(
          "flex list-none items-center gap-1.5 py-0.5 text-xs transition-colors [&::-webkit-details-marker]:hidden",
          isError ? "text-destructive" : "text-muted-foreground hover:text-foreground",
        )}
      >
        <ChevronRightIcon className="size-3 shrink-0 transition-transform group-open/tool:rotate-90" />
        <span className="truncate font-mono">{toolTitle(toolName, args)}</span>
        {action && <span className="truncate opacity-70">{action}</span>}
        {isError && <span className="shrink-0">失败</span>}
      </summary>
      {(detail || output) && (
        <div className="mt-1 mb-1.5 ml-1.5 grid gap-1.5 border-l border-border pl-3 text-xs text-muted-foreground">
          {detail && <pre className="break-all whitespace-pre-wrap">{detail}</pre>}
          {output && (
            <pre className="max-h-64 overflow-auto break-all whitespace-pre-wrap">{output}</pre>
          )}
        </div>
      )}
    </details>
  );
}
