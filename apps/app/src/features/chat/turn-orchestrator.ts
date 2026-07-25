// One chat turn, end to end, with no React in sight.
//
// A turn is a single HTTP request that can block server-side for minutes: the
// gateway suspends it when a tool needs approval or the agent asks a question,
// and both are resolved out-of-band. So while the request is in flight we poll
// the interactions endpoint and surface whatever it reports; the same request
// eventually returns the final reply. Tool-call frames arrive live on the
// stream and fold into an activity list.
//
// Everything here is injectable (client + sleep), which is what makes the
// timing behaviour testable — see turn-orchestrator.test.ts.

import type { KomoClient, TurnEvent } from "@/shared/api/types";
import { INTERACTIONS_BACKOFF_MS, POLL } from "@/shared/config";
import { sleep as realSleep } from "@/shared/lib/async";
import { headerFor } from "@/shared/lib/session-id";
import type { Interactions, Mode, PendingApproval } from "@/shared/types";
import { interactionsPath } from "./api";

/** One tool call as the live feed knows it. */
export interface ToolActivity {
  seq: number;
  name: string;
  args: string;
  done: boolean;
  ok?: boolean;
  summary?: string;
}

/** Fold one streamed event into the activity list. Pure: a started event
 *  replaces any earlier entry with the same seq, a finished event marks it. */
export function foldToolEvent(tools: ToolActivity[], event: TurnEvent): ToolActivity[] {
  if (event.type === "tool_started") {
    return [
      ...tools.filter((t) => t.seq !== event.seq),
      { seq: event.seq, name: event.name, args: event.args, done: false },
    ];
  }
  return tools.map((t) =>
    t.seq === event.seq ? { ...t, done: true, ok: event.ok, summary: event.summary } : t,
  );
}

export interface TurnHooks {
  /** The whole activity list, on every change. */
  onTools?: (tools: ToolActivity[]) => void;
  onApproval?: (approval: PendingApproval | null) => void;
  onQuestion?: (question: string | null) => void;
}

export interface TurnDeps {
  client: KomoClient;
  /** Interval between interaction polls (overridden in tests). */
  pollMs?: number;
  /** Abortable delay (overridden in tests). */
  sleep?: (ms: number, signal?: AbortSignal) => Promise<void>;
}

export interface TurnRequest {
  session: string;
  message: string;
  mode: Mode;
}

export interface TurnResult {
  reply: string;
  tools: ToolActivity[];
}

/** Poll for pending approvals/questions until aborted. A single failure is
 *  transient (the gateway is busy), so back off and keep going; only an
 *  exhausted backoff gives up — dropping the poll silently would leave an
 *  approval prompt invisible for the rest of the turn. */
async function pollInteractions(
  session: string,
  hooks: TurnHooks,
  deps: Required<Pick<TurnDeps, "client" | "pollMs" | "sleep">>,
  signal: AbortSignal,
): Promise<void> {
  const path = interactionsPath(session);
  let failures = 0;
  while (!signal.aborted) {
    const res = await deps.client.api<Interactions>({ path });
    if (signal.aborted) return;
    if (res.ok && res.data) {
      failures = 0;
      hooks.onApproval?.(res.data.approval ?? null);
      hooks.onQuestion?.(res.data.question ?? null);
    } else if (++failures > INTERACTIONS_BACKOFF_MS.length) {
      return;
    }
    const delay = failures === 0 ? deps.pollMs : INTERACTIONS_BACKOFF_MS[failures - 1];
    await deps.sleep(delay, signal);
  }
}

/** Run one turn. Throws when the request itself fails; tool errors are part of
 *  the returned activity list, not exceptions. */
export async function runTurn(
  req: TurnRequest,
  hooks: TurnHooks,
  deps: TurnDeps,
): Promise<TurnResult> {
  const resolved = {
    client: deps.client,
    pollMs: deps.pollMs ?? POLL.interactions,
    sleep: deps.sleep ?? realSleep,
  };

  let tools: ToolActivity[] = [];
  hooks.onTools?.(tools);

  const controller = new AbortController();
  const poll = pollInteractions(req.session, hooks, resolved, controller.signal).catch(() => {
    /* a poll must never fail the turn */
  });

  try {
    const res = await resolved.client.chat(
      {
        header: headerFor(req.session),
        message: req.message,
        mode: req.mode,
      },
      (event) => {
        tools = foldToolEvent(tools, event);
        hooks.onTools?.(tools);
      },
    );
    if (!res.ok) throw new Error(res.error || "请求失败");
    return { reply: res.reply ?? "", tools };
  } finally {
    controller.abort();
    await poll;
    hooks.onApproval?.(null);
    hooks.onQuestion?.(null);
  }
}
