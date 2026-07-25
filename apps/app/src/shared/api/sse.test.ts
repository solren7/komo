import { describe, expect, it } from "vitest";

import { createFrameSplitter, parseFrame, textDeltaFrom, toolEventFrom } from "./sse";

const chunk = (content: string) =>
  `data: ${JSON.stringify({ choices: [{ delta: { content } }] })}\n\n`;

describe("parseFrame", () => {
  it("defaults the event name to message", () => {
    expect(parseFrame("data: hi")).toEqual({ event: "message", data: "hi" });
  });

  it("reads the event name and strips one leading space from data", () => {
    expect(parseFrame("event: tool\ndata: {}")).toEqual({ event: "tool", data: "{}" });
  });

  it("joins multi-line data payloads", () => {
    expect(parseFrame("data: a\ndata: b")?.data).toBe("a\nb");
  });

  it("drops payload-less frames and the [DONE] sentinel", () => {
    expect(parseFrame(": keep-alive")).toBeNull();
    expect(parseFrame("data: [DONE]")).toBeNull();
  });
});

describe("createFrameSplitter", () => {
  it("yields nothing until a frame is complete", () => {
    const splitter = createFrameSplitter();
    expect(splitter.push("data: par")).toEqual([]);
    expect(splitter.push("tial\n\n")).toEqual([{ event: "message", data: "partial" }]);
  });

  it("splits several frames arriving in one chunk", () => {
    const splitter = createFrameSplitter();
    expect(splitter.push(chunk("a") + chunk("b"))).toHaveLength(2);
  });

  it("survives a chunk boundary inside the JSON payload", () => {
    const splitter = createFrameSplitter();
    const frame = chunk("hello");
    const cut = Math.floor(frame.length / 2);
    const first = splitter.push(frame.slice(0, cut));
    const rest = splitter.push(frame.slice(cut));
    expect(first).toEqual([]);
    expect(rest.map(textDeltaFrom)).toEqual(["hello"]);
  });

  it("flushes a trailing frame with no blank line", () => {
    const splitter = createFrameSplitter();
    splitter.push("data: tail");
    expect(splitter.flush()).toEqual([{ event: "message", data: "tail" }]);
  });

  it("flushes nothing for trailing whitespace", () => {
    const splitter = createFrameSplitter();
    splitter.push("\n");
    expect(splitter.flush()).toEqual([]);
  });
});

describe("frame interpretation", () => {
  it("parses a tool frame into a TurnEvent", () => {
    const frame = {
      event: "tool",
      data: '{"type":"tool_started","seq":1,"name":"shell","args":"{}"}',
    };
    expect(toolEventFrom(frame)).toEqual({
      type: "tool_started",
      seq: 1,
      name: "shell",
      args: "{}",
    });
  });

  it("ignores a malformed tool frame instead of throwing", () => {
    expect(toolEventFrom({ event: "tool", data: "{not json" })).toBeNull();
  });

  it("keeps tool and text frames apart", () => {
    expect(toolEventFrom({ event: "message", data: "{}" })).toBeNull();
    expect(textDeltaFrom({ event: "tool", data: "{}" })).toBeNull();
  });

  it("extracts a text delta and tolerates chunks without one", () => {
    expect(
      textDeltaFrom({ event: "message", data: '{"choices":[{"delta":{"content":"hi"}}]}' }),
    ).toBe("hi");
    expect(textDeltaFrom({ event: "message", data: '{"choices":[{"delta":{}}]}' })).toBeNull();
    expect(textDeltaFrom({ event: "message", data: "nope" })).toBeNull();
  });
});
