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
 *  parses, for skill-name detection in the view.
 *
 *  `timing` drives `useToolCallElapsed`, which wants epoch millis. The ledger
 *  only has whole seconds plus the measured `elapsed_ms`, so the pair is
 *  reconstructed from those — and omitted entirely when `elapsed_ms` is 0 (a
 *  step recorded before the column existed: unknown, not instant). */
export function toolPart(
  step: Pick<RunStep, "seq" | "tool_name" | "args" | "result" | "error" | "ok"> &
    Partial<Pick<RunStep, "elapsed_ms">> & { started_at_ms?: number },
) {
  const startedAt = step.started_at_ms;
  const elapsed = step.elapsed_ms;
  return {
    type: "tool-call" as const,
    toolCallId: `tool-${step.seq}`,
    toolName: step.tool_name,
    args: parseArgs(step.args) ?? {},
    argsText: step.args,
    result: step.ok ? step.result : step.error,
    isError: !step.ok,
    ...(startedAt !== undefined && elapsed
      ? { timing: { startedAt, completedAt: startedAt + elapsed } }
      : {}),
  };
}

/** A ledger step → a finished tool part. `started_at` is seconds; scaling it to
 *  millis is what makes the reconstructed `timing` line up with the live feed's. */
export function stepToolPart(step: RunStep & { started_at?: number }) {
  return toolPart({
    ...step,
    started_at_ms: step.started_at === undefined ? undefined : step.started_at * 1000,
  });
}

/** The live feed's view of a call → the same message part, so a call renders
 *  identically while it runs, once it lands, and after a reload.
 *
 *  A *running* call is the one difference, and it is a deliberate one: `result`
 *  stays `undefined` so assistant-ui reports the part's status as `running`, and
 *  `timing` carries a start with no completion so the duration ticks. */
export function activityToolPart(tool: ToolActivity) {
  const base = {
    type: "tool-call" as const,
    toolCallId: `tool-${tool.seq}`,
    toolName: tool.name,
    args: parseArgs(tool.args) ?? {},
    argsText: tool.args,
    timing: {
      startedAt: tool.startedAtMs,
      ...(tool.elapsedMs === undefined ? {} : { completedAt: tool.startedAtMs + tool.elapsedMs }),
    },
  };
  if (!tool.done) return base;
  const ok = tool.ok ?? false;
  return {
    ...base,
    result: ok ? (tool.summary ?? "") : (tool.summary ?? "调用失败"),
    isError: !ok,
  };
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
          ...(pending?.steps ?? []).map(stepToolPart),
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
