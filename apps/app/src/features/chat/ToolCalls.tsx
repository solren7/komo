// How a turn's tool calls read, both while they run and afterwards.
//
// A turn can spend a dozen calls before it answers and none of them is the
// answer, so the whole round collapses to one line — `3 次工具调用  shell ·
// time` — which expands to the list of calls, each of which expands again to its
// own arguments and result. Deliberately chrome-less at every level: no card, no
// fill, the disclosure arrow is the only affordance.
//
// The chrome itself (Collapsible mechanics, the open/close animation, the
// running shimmer, the status icon, the ticking duration) is the vendored
// assistant-ui kit in shared/ui/tool-group.tsx + tool-fallback.tsx. This file is
// only what komo says on those lines: the Chinese copy, the tool names on the
// collapsed line, the skill a `skill` call loaded.
//
// There is no separate live view. A running call is the same component with a
// running part — see history.ts::activityToolPart and the streaming adapter in
// ChatView — so nothing re-renders or jumps when the turn lands.

import { type PropsWithChildren, useMemo } from "react";
import { useAuiState, type ToolCallMessagePartProps } from "@assistant-ui/react";

import { cn } from "@/shared/lib/utils";
import {
  ToolFallbackArgs,
  ToolFallbackContent,
  ToolFallbackResult,
  ToolFallbackRoot,
  ToolFallbackTrigger,
} from "@/shared/ui/tool-fallback";
import { ToolGroupContent, ToolGroupRoot, ToolGroupTrigger } from "@/shared/ui/tool-group";
import {
  summarizeToolRound,
  toolTitle,
  type ToolRoundCall,
  type ToolRoundSummary,
} from "./tool-summary";

/** Adjacent tool calls, collapsed to one line.
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
  // the collapsed line can name the tools that ran and say what is happening.
  const parts = useAuiState((state) => state.message.parts);
  const { summary, running } = useMemo(() => {
    const calls: ToolRoundCall[] = [];
    let anyRunning = false;
    for (const index of indices) {
      const part = parts[index];
      const call = part?.type === "tool-call" ? part : undefined;
      if (part?.status?.type === "running") anyRunning = true;
      calls.push({ name: call?.toolName ?? "tool", failed: call?.isError === true });
    }
    return { summary: summarizeToolRound(calls), running: anyRunning };
  }, [indices, parts]);

  return (
    <ToolGroupRoot variant="ghost" className="my-0.5">
      <ToolGroupTrigger
        count={summary.count}
        active={running}
        // While calls are still landing the count is only the count *so far*,
        // which ticks upward and reads like a glitch. Say what is happening
        // instead, and let the tool names carry the detail.
        label={running ? "正在调用" : `${summary.count} 次工具调用`}
        // `!`: the variant's own `group-data-[variant=ghost]:text-muted-foreground`
        // is a different selector, so plain `text-destructive` would not
        // reliably win the cascade.
        className={cn(summary.failed > 0 && "text-destructive!")}
      >
        <ToolRoundDetail summary={summary} />
      </ToolGroupTrigger>
      <ToolGroupContent>{children}</ToolGroupContent>
    </ToolGroupRoot>
  );
}

/** What the collapsed round line carries beyond its label: the failure count,
 *  then the tools that ran. */
function ToolRoundDetail({ summary }: { summary: ToolRoundSummary }) {
  return (
    <>
      {summary.failed > 0 && <span className="shrink-0 text-xs">· {summary.failed} 个失败</span>}
      <span className="truncate font-mono text-xs opacity-70">{summary.names}</span>
    </>
  );
}

/** One tool call: a quiet line that expands in place to the arguments and the
 *  (truncated) result.
 *
 *  komo reports a tool failure as `isError` on the part, not as a part status —
 *  the call itself completed, it is the tool that failed. The status is
 *  synthesized here so the kit picks its failure icon, and the message is left
 *  in `result` (which is where the gateway puts it) rather than duplicated into
 *  `status.error`. */
export function ToolCallView({
  toolName,
  args,
  argsText,
  result,
  isError,
  status,
}: ToolCallMessagePartProps) {
  const action = toolName === "skill" && typeof args?.action === "string" ? args.action : null;
  const detail = argsText || (args ? JSON.stringify(args, null, 2) : "");
  const output =
    typeof result === "string" ? result : result == null ? "" : JSON.stringify(result, null, 2);

  return (
    <ToolFallbackRoot className="my-0.5">
      <ToolFallbackTrigger
        toolName={toolName}
        status={isError ? { type: "incomplete", reason: "error" } : status}
        label={toolTitle(toolName, args)}
        detail={action && <span className="truncate text-xs opacity-70">{action}</span>}
        className={cn("font-mono text-xs", isError && "text-destructive")}
      />
      <ToolFallbackContent>
        <ToolFallbackArgs argsText={detail} />
        <ToolFallbackResult result={output || undefined} header={isError ? "错误：" : "结果："} />
      </ToolFallbackContent>
    </ToolFallbackRoot>
  );
}
