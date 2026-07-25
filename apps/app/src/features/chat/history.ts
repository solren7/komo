// Re-hydrating a past session: the transcript comes from the message store, the
// tool calls from the run ledger, and this module stitches them back into the
// shape assistant-ui wants.

import type { ThreadMessageLike } from "@assistant-ui/react";

import type { RunDetail, RunStep, SessionMessage } from "@/shared/types";
import { fetchRunDetail, fetchRuns, fetchSessionMessages } from "./api";
import type { ToolActivity } from "./turn-orchestrator";

type JsonValue = string | number | boolean | null | JsonValue[] | { [k: string]: JsonValue };

export function parseArgs(raw: string): Record<string, JsonValue> | undefined {
  try {
    const value = JSON.parse(raw);
    return value && typeof value === "object" && !Array.isArray(value)
      ? (value as Record<string, JsonValue>)
      : undefined;
  } catch {
    return undefined;
  }
}

/** A RunStep → an assistant tool-call message part (rendered by `ToolCallView`).
 *  `argsText` is always the raw JSON; `args` is the parsed object when it
 *  parses, for skill-name detection in the view. */
export function toolPart(
  step: Pick<RunStep, "seq" | "tool_name" | "args" | "result" | "error" | "ok">,
) {
  return {
    type: "tool-call" as const,
    toolCallId: `tool-${step.seq}`,
    toolName: step.tool_name,
    args: parseArgs(step.args) ?? {},
    argsText: step.args,
    result: step.ok ? step.result : step.error,
    isError: !step.ok,
  };
}

/** The live feed's view of a call → the same message part, so a just-finished
 *  turn renders identically to a re-hydrated one. */
export function activityToolPart(tool: ToolActivity) {
  return toolPart({
    seq: tool.seq,
    tool_name: tool.name,
    args: tool.args,
    result: tool.ok ? (tool.summary ?? "") : "",
    error: tool.ok ? "" : (tool.summary ?? "调用失败"),
    ok: tool.ok ?? false,
  });
}

/** Pair each user message with the run it started.
 *
 *  Runs are matched by input text first and only then by order, because order
 *  alone skews: a turn that failed before producing an assistant message (or a
 *  transcript the server windowed) shifts every later run onto the wrong
 *  message, silently attributing tool calls to the wrong question. */
export function buildInitialMessages(
  messages: SessionMessage[],
  details: RunDetail[],
): ThreadMessageLike[] {
  const runs = [...details].sort((a, b) => a.run.started_at - b.run.started_at);
  const used = new Set<number>();
  const claim = (input: string): RunDetail | undefined => {
    const exact = runs.findIndex((d, i) => !used.has(i) && d.run.input === input);
    const idx = exact >= 0 ? exact : runs.findIndex((_, i) => !used.has(i));
    if (idx < 0) return undefined;
    used.add(idx);
    return runs[idx];
  };

  const result: ThreadMessageLike[] = [];
  let pending: RunDetail | undefined;
  for (const message of messages) {
    if (message.role === "user") {
      pending = claim(message.content);
      result.push({
        role: "user",
        content: message.content,
        createdAt: new Date(message.timestamp * 1000),
      });
    } else if (message.role === "assistant") {
      result.push({
        role: "assistant",
        content: [
          ...(pending?.steps ?? []).map(toolPart),
          { type: "text" as const, text: message.content },
        ],
        createdAt: new Date(message.timestamp * 1000),
      });
      pending = undefined;
    }
  }
  return result;
}

/** Load one session's transcript, with its runs' tool calls folded in. */
export async function loadSessionHistory(session: string): Promise<ThreadMessageLike[]> {
  const [messages, runs] = await Promise.all([fetchSessionMessages(session), fetchRuns()]);
  const mine = runs
    .filter((run) => run.session_id === session)
    .sort((a, b) => a.started_at - b.started_at);
  const details = await Promise.all(mine.map((run) => fetchRunDetail(run.id)));
  return buildInitialMessages(messages, details);
}
