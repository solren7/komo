import { describe, expect, it } from "vitest";

import type { RunDetail, RunStep, SessionMessage } from "@/shared/types";
import { activityToolPart, buildInitialMessages, parseArgs, toolPart } from "./history";

const step = (over: Partial<RunStep> = {}): RunStep => ({
  seq: 1,
  tool_name: "shell",
  args: '{"cmd":"ls"}',
  result: "ok",
  error: "",
  ok: true,
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
    const live = activityToolPart({
      seq: 3,
      name: "time",
      args: "{}",
      done: true,
      ok: true,
      summary: "now",
    });
    expect(live).toMatchObject({ toolName: "time", result: "now", isError: false });
  });

  it("gives a failed live call a fallback error message", () => {
    const live = activityToolPart({ seq: 4, name: "shell", args: "{}", done: true, ok: false });
    expect(live).toMatchObject({ isError: true, result: "调用失败" });
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
