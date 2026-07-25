import { describe, expect, it } from "vitest";

import { pushStream } from "./async";

/** Drain an iterable into an array. */
async function collect<T>(source: AsyncIterable<T>): Promise<T[]> {
  const out: T[] = [];
  for await (const value of source) out.push(value);
  return out;
}

describe("pushStream", () => {
  it("yields values pushed before iteration starts", async () => {
    const s = pushStream<number>();
    s.push(1);
    s.close();
    expect(await collect(s)).toEqual([1]);
  });

  it("wakes a waiting consumer when a value arrives", async () => {
    const s = pushStream<string>();
    const drained = collect(s);
    // The consumer is parked on an empty stream at this point.
    await Promise.resolve();
    s.push("a");
    s.push("b");
    s.close();
    expect(await drained).toEqual(["b"]);
  });

  // The contract that makes it safe for a tool feed: each push is a whole
  // snapshot, so a consumer slower than the producer should jump to the current
  // state rather than replay every superseded one.
  it("coalesces a burst to the latest value", async () => {
    const s = pushStream<number>();
    s.push(1);
    s.push(2);
    s.push(3);
    s.close();
    expect(await collect(s)).toEqual([3]);
  });

  it("drains what is pending before honouring a close", async () => {
    const s = pushStream<number>();
    s.push(7);
    s.close();
    // Close does not discard the value already queued.
    expect(await collect(s)).toEqual([7]);
  });

  it("ignores a push after close", async () => {
    const s = pushStream<number>();
    s.close();
    s.push(9);
    expect(await collect(s)).toEqual([]);
  });

  // An interrupted turn abandons the loop early; neither side may then hang or
  // throw. `close` with nobody listening is the producer's half of that.
  it("survives a consumer that abandons iteration, and a close with no listener", async () => {
    const s = pushStream<number>();
    s.push(1);
    for await (const value of s) {
      expect(value).toBe(1);
      break;
    }
    expect(() => {
      s.push(2);
      s.close();
    }).not.toThrow();
  });

  it("terminates when closed while a consumer is waiting", async () => {
    const s = pushStream<number>();
    const drained = collect(s);
    await Promise.resolve();
    s.close();
    expect(await drained).toEqual([]);
  });
});
