import { describe, expect, it } from "vitest";

import type { RunDetail, RunStep, SessionMessage } from "@/shared/types";
import {
  activityToolPart,
  buildInitialMessages,
  parseArgs,
  stepToolPart,
  toolPart,
} from "./history";
import type { ToolActivity } from "./turn-orchestrator";

const step = (over: Partial<RunStep> = {}): RunStep => ({
  seq: 1,
  tool_name: "shell",
  args: '{"cmd":"ls"}',
  result: "ok",
  error: "",
  ok: true,
  elapsed_ms: 40,
  ...over,
});

/** A live activity, defaulted to a finished successful call. */
const activity = (over: Partial<ToolActivity> = {}): ToolActivity => ({
  seq: 3,
  name: "time",
  args: "{}",
  done: true,
  ok: true,
  summary: "now",
  startedAtMs: 1_700_000_000_000,
  elapsedMs: 40,
  ...over,
});

const run = (id: string, input: string, startedAt: number, steps: RunStep[]): RunDetail => ({
  run: {
    id,
    session_id: "s",
    input,
    plan: "",
    status: "done",
    recoverable: false,
    started_at: startedAt,
    ended_at: startedAt + 1,
    final_output: "",
    error: "",
  },
  steps,
});

const user = (content: string, ts = 1): SessionMessage => ({
  role: "user",
  content,
  timestamp: ts,
});
const assistant = (content: string, ts = 2): SessionMessage => ({
  role: "assistant",
  content,
  timestamp: ts,
});

describe("parseArgs", () => {
  it("parses an object", () => {
    expect(parseArgs('{"a":1}')).toEqual({ a: 1 });
  });

  it("rejects non-objects and garbage", () => {
    expect(parseArgs("[1,2]")).toBeUndefined();
    expect(parseArgs("null")).toBeUndefined();
    expect(parseArgs("not json")).toBeUndefined();
  });
});

describe("toolPart", () => {
  it("keeps the raw args text even when it doesn't parse", () => {
    const part = toolPart(step({ args: "{broken" }));
    expect(part.argsText).toBe("{broken");
    expect(part.args).toEqual({});
  });

  it("reports the error as the result for a failed call", () => {
    const part = toolPart(step({ ok: false, error: "boom", result: "" }));
    expect(part).toMatchObject({ isError: true, result: "boom" });
  });

  it("maps a live activity the same way a ledger step maps", () => {
    const live = activityToolPart(activity());
    expect(live).toMatchObject({ toolName: "time", result: "now", isError: false });
  });

  it("gives a failed live call a fallback error message", () => {
    const live = activityToolPart(
      activity({ seq: 4, name: "shell", ok: false, summary: undefined }),
    );
    expect(live).toMatchObject({ isError: true, result: "调用失败" });
  });

  // A running call is the one part that differs, and both differences matter:
  // no `result` is what makes assistant-ui report the part as running, and a
  // `timing` without `completedAt` is what makes the duration tick.
  it("leaves a running call without a result and without a completion time", () => {
    const live = activityToolPart(
      activity({ done: false, ok: undefined, summary: undefined, elapsedMs: undefined }),
    );
    expect(live).not.toHaveProperty("result");
    expect(live).not.toHaveProperty("isError");
    expect(live.timing).toEqual({ startedAt: 1_700_000_000_000 });
  });

  it("derives a finished call's timing window from the measured duration", () => {
    const live = activityToolPart(activity({ startedAtMs: 1000, elapsedMs: 250 }));
    expect(live.timing).toEqual({ startedAt: 1000, completedAt: 1250 });
  });
});

describe("stepToolPart", () => {
  // The ledger keeps whole seconds; assistant-ui's timing wants epoch millis.
  it("scales the ledger's second-resolution start into the timing window", () => {
    const part = stepToolPart({ ...step(), started_at: 1_700_000_000, elapsed_ms: 250 });
    expect(part.timing).toEqual({
      startedAt: 1_700_000_000_000,
      completedAt: 1_700_000_000_250,
    });
  });

  // Steps predate the column; 0 means unknown, and claiming "instant" would be
  // a lie the UI renders as `<1s`.
  it("omits timing entirely when the duration was never recorded", () => {
    const part = stepToolPart({ ...step(), started_at: 1_700_000_000, elapsed_ms: 0 });
    expect(part).not.toHaveProperty("timing");
  });

  // A gateway predating the field omits it rather than sending 0 — the same
  // "unknown", arriving a different way.
  it("omits timing when the gateway never sent the field", () => {
    const { elapsed_ms: _drop, ...legacy } = step();
    const part = stepToolPart({ ...legacy, started_at: 1_700_000_000 });
    expect(part).not.toHaveProperty("timing");
  });
});

describe("buildInitialMessages", () => {
  it("attaches each run's steps to the assistant reply that followed it", () => {
    const messages = [
      user("one", 10),
      assistant("first", 11),
      user("two", 20),
      assistant("second", 21),
    ];
    const details = [
      run("r1", "one", 10, [step({ seq: 1 })]),
      run("r2", "two", 20, [step({ seq: 2 })]),
    ];
    const thread = buildInitialMessages(messages, details);
    expect(thread).toHaveLength(4);
    expect(thread[1].content).toMatchObject([{ toolCallId: "tool-1" }, { text: "first" }]);
    expect(thread[3].content).toMatchObject([{ toolCallId: "tool-2" }, { text: "second" }]);
  });

  it("orders runs by start time, not by the order they arrived", () => {
    const messages = [
      user("one", 10),
      assistant("first", 11),
      user("two", 20),
      assistant("second", 21),
    ];
    const details = [
      run("r2", "two", 20, [step({ seq: 2 })]),
      run("r1", "one", 10, [step({ seq: 1 })]),
    ];
    const thread = buildInitialMessages(messages, details);
    expect(thread[1].content).toMatchObject([{ toolCallId: "tool-1" }, { text: "first" }]);
  });

  it("still matches by input when a turn produced no assistant message", () => {
    // "two" failed: it has a run but no reply. Index-order pairing would hand
    // run "two" to the third question and skew everything after it.
    const messages = [
      user("one", 10),
      assistant("first", 11),
      user("two", 20),
      user("three", 30),
      assistant("third", 31),
    ];
    const details = [
      run("r1", "one", 10, [step({ seq: 1 })]),
      run("r2", "two", 20, [step({ seq: 2 })]),
      run("r3", "three", 30, [step({ seq: 3 })]),
    ];
    const thread = buildInitialMessages(messages, details);
    expect(thread[1].content).toMatchObject([{ toolCallId: "tool-1" }, { text: "first" }]);
    expect(thread[4].content).toMatchObject([{ toolCallId: "tool-3" }, { text: "third" }]);
  });

  it("renders replies with no run at all", () => {
    const thread = buildInitialMessages([user("hi"), assistant("hello")], []);
    expect(thread[1].content).toMatchObject([{ text: "hello" }]);
  });

  it("skips system and tool messages", () => {
    const messages: SessionMessage[] = [
      { role: "system", content: "sys", timestamp: 1 },
      user("hi", 2),
      { role: "tool", content: "raw", timestamp: 3 },
      assistant("hello", 4),
    ];
    expect(buildInitialMessages(messages, [])).toHaveLength(2);
  });
});
