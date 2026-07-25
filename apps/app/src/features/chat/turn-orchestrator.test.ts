import { describe, expect, it } from "vitest";

import type { KomoApiResponse, KomoChatRequest, KomoClient, TurnEvent } from "@/shared/api/types";
import type { Interactions, PendingApproval } from "@/shared/types";
import { foldToolEvent, runTurn, type ToolActivity } from "./turn-orchestrator";

const started = (seq: number, name = "shell"): TurnEvent => ({
  type: "tool_started",
  seq,
  name,
  args: '{"cmd":"ls"}',
});
const finished = (seq: number, ok = true): TurnEvent => ({
  type: "tool_finished",
  seq,
  name: "shell",
  ok,
  summary: ok ? "done" : "failed",
});

const approval: PendingApproval = { summary: "run ls", detail: null, risk: "normal" };

/** A client whose chat() resolves only when the test says so, and whose api()
 *  replays a scripted sequence of interaction responses. */
function harness(options: {
  interactions?: KomoApiResponse<Interactions>[];
  onChat?: (emit: (event: TurnEvent) => void) => Promise<void>;
  chatResult?: { ok: boolean; reply?: string; error?: string };
}) {
  const script = [...(options.interactions ?? [])];
  let polls = 0;
  const client: KomoClient = {
    connect: async () => ({ connected: true }),
    api: async () => {
      polls++;
      const next = script.shift() ?? {
        ok: true,
        status: 200,
        data: { approval: null, question: null },
      };
      return next as KomoApiResponse<never>;
    },
    chat: async (_req: KomoChatRequest, onToolEvent) => {
      await options.onChat?.((event) => onToolEvent?.(event));
      return options.chatResult ?? { ok: true, reply: "hi" };
    },
  };
  return { client, polls: () => polls };
}

/** A sleep the test drives by hand: the poll loop advances exactly one
 *  iteration per `tick()`, so nothing spins while the turn is gated. */
function controlledClock() {
  let waiters: (() => void)[] = [];
  const sleep = (_ms: number, signal?: AbortSignal) => {
    if (signal?.aborted) return Promise.resolve();
    return new Promise<void>((resolve) => {
      waiters.push(resolve);
      signal?.addEventListener("abort", () => resolve(), { once: true });
    });
  };
  /** Release every pending sleep, then let the awakened code run. */
  const tick = async () => {
    const pending = waiters;
    waiters = [];
    for (const wake of pending) wake();
    await flush();
  };
  return { sleep, tick };
}

/** Drain pending microtasks (a poll's `await client.api(...)`). */
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("foldToolEvent", () => {
  it("appends a started call", () => {
    expect(foldToolEvent([], started(1))).toEqual([
      { seq: 1, name: "shell", args: '{"cmd":"ls"}', done: false },
    ]);
  });

  it("marks the matching call finished, leaving others alone", () => {
    const tools = foldToolEvent(foldToolEvent([], started(1)), started(2, "time"));
    const done = foldToolEvent(tools, finished(1));
    expect(done[0]).toMatchObject({ seq: 1, done: true, ok: true, summary: "done" });
    expect(done[1]).toMatchObject({ seq: 2, done: false });
  });

  it("replaces a re-started seq rather than duplicating it", () => {
    const tools = foldToolEvent(foldToolEvent([], started(1)), started(1, "time"));
    expect(tools).toHaveLength(1);
    expect(tools[0].name).toBe("time");
  });

  it("ignores a finish for an unknown seq", () => {
    expect(foldToolEvent([], finished(9))).toEqual([]);
  });
});

describe("runTurn", () => {
  it("returns the reply and the calls made during the turn", async () => {
    const { client } = harness({
      onChat: async (emit) => {
        emit(started(1));
        emit(finished(1));
      },
    });
    const seen: ToolActivity[][] = [];
    const result = await runTurn(
      { session: "api:gui-web-1", message: "hi", mode: "interactive" },
      { onTools: (tools) => seen.push(tools) },
      { client, sleep: controlledClock().sleep },
    );
    expect(result.reply).toBe("hi");
    expect(result.tools).toMatchObject([{ seq: 1, done: true, ok: true }]);
    // Reset, started, finished — the strip updates live.
    expect(seen.map((t) => t.length)).toEqual([0, 1, 1]);
  });

  it("strips the api: prefix before sending the session header", async () => {
    let header = "";
    const client: KomoClient = {
      connect: async () => ({ connected: true }),
      api: async () =>
        ({ ok: true, status: 200, data: { approval: null, question: null } }) as never,
      chat: async (req) => {
        header = req.header;
        return { ok: true, reply: "" };
      },
    };
    await runTurn(
      { session: "api:gui-desktop-9", message: "hi", mode: "trusted" },
      {},
      { client, sleep: controlledClock().sleep },
    );
    expect(header).toBe("gui-desktop-9");
  });

  it("surfaces a pending approval while the turn is in flight, then clears it", async () => {
    let release = () => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const clock = controlledClock();
    const { client } = harness({
      interactions: [{ ok: true, status: 200, data: { approval, question: null } }],
      onChat: async () => gate,
    });
    const approvals: (PendingApproval | null)[] = [];
    const turn = runTurn(
      { session: "s", message: "hi", mode: "interactive" },
      { onApproval: (a) => approvals.push(a) },
      { client, sleep: clock.sleep },
    );
    await flush();
    expect(approvals).toEqual([approval]);
    release();
    await turn;
    expect(approvals.at(-1)).toBeNull();
  });

  it("surfaces a clarify question the same way", async () => {
    let release = () => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const clock = controlledClock();
    const { client } = harness({
      interactions: [{ ok: true, status: 200, data: { approval: null, question: "哪个环境？" } }],
      onChat: async () => gate,
    });
    const questions: (string | null)[] = [];
    const turn = runTurn(
      { session: "s", message: "hi", mode: "interactive" },
      { onQuestion: (q) => questions.push(q) },
      { client, sleep: clock.sleep },
    );
    await flush();
    expect(questions).toEqual(["哪个环境？"]);
    release();
    await turn;
    expect(questions.at(-1)).toBeNull();
  });

  it("keeps polling after a transient interaction failure", async () => {
    let release = () => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const clock = controlledClock();
    const { client } = harness({
      interactions: [
        { ok: false, status: 0, error: "网络抖动" },
        { ok: true, status: 200, data: { approval, question: null } },
      ],
      onChat: async () => gate,
    });
    const approvals: (PendingApproval | null)[] = [];
    const turn = runTurn(
      { session: "s", message: "hi", mode: "interactive" },
      { onApproval: (a) => approvals.push(a) },
      { client, sleep: clock.sleep },
    );
    await flush();
    expect(approvals).toEqual([]);
    // The failure must not kill the loop — otherwise a prompt raised after it
    // would never reach the user and the turn would hang until the server
    // timeout.
    await clock.tick();
    expect(approvals).toEqual([approval]);
    release();
    await turn;
  });

  it("gives up polling once the backoff is exhausted", async () => {
    let release = () => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const clock = controlledClock();
    const { client, polls } = harness({
      interactions: Array.from({ length: 30 }, () => ({
        ok: false as const,
        status: 0,
        error: "down",
      })),
      onChat: async () => gate,
    });
    const turn = runTurn(
      { session: "s", message: "hi", mode: "interactive" },
      {},
      { client, sleep: clock.sleep },
    );
    await flush();
    for (let i = 0; i < 10; i++) await clock.tick();
    // 5 backoff steps + the poll that exhausted them.
    expect(polls()).toBe(6);
    release();
    await turn;
  });

  it("throws when the request fails, and still stops polling", async () => {
    const clock = controlledClock();
    const { client, polls } = harness({ chatResult: { ok: false, error: "HTTP 500" } });
    await expect(
      runTurn(
        { session: "s", message: "hi", mode: "interactive" },
        {},
        { client, sleep: clock.sleep },
      ),
    ).rejects.toThrow("HTTP 500");
    const after = polls();
    await clock.tick();
    expect(polls()).toBe(after);
  });

  it("reports a failed tool call in the result instead of throwing", async () => {
    const { client } = harness({
      onChat: async (emit) => {
        emit(started(1));
        emit(finished(1, false));
      },
    });
    const result = await runTurn(
      { session: "s", message: "hi", mode: "interactive" },
      {},
      { client, sleep: controlledClock().sleep },
    );
    expect(result.tools[0]).toMatchObject({ done: true, ok: false, summary: "failed" });
  });
});
