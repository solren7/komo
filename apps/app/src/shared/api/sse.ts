// SSE wire format, as pure functions. The gateway's chat stream carries two
// kinds of frame on one connection — OpenAI-style completion chunks (the
// assistant text) and komo's own `event: tool` frames — so splitting and
// interpreting them is real logic worth testing away from the network.

import type { TurnEvent } from "./types";

export interface SseFrame {
  /** The `event:` field, or "message" when absent. */
  event: string;
  data: string;
}

/** Parse one raw frame (the text between blank lines). Returns null for frames
 *  with no payload and for the terminal `[DONE]` sentinel. */
export function parseFrame(raw: string): SseFrame | null {
  let event = "message";
  const dataLines: string[] = [];
  for (const line of raw.split("\n")) {
    if (line.startsWith("event:")) event = line.slice(6).trim();
    else if (line.startsWith("data:")) dataLines.push(line.slice(5).replace(/^ /, ""));
  }
  const data = dataLines.join("\n");
  if (!data || data === "[DONE]") return null;
  return { event, data };
}

/** Accumulates decoded chunks and yields whole frames as they complete — a
 *  network chunk can split a frame anywhere, including mid-JSON. */
export function createFrameSplitter() {
  let buf = "";
  return {
    push(chunk: string): SseFrame[] {
      buf += chunk;
      const frames: SseFrame[] = [];
      for (;;) {
        const idx = buf.indexOf("\n\n");
        if (idx < 0) break;
        const frame = parseFrame(buf.slice(0, idx));
        buf = buf.slice(idx + 2);
        if (frame) frames.push(frame);
      }
      return frames;
    },
    /** Whatever is left when the stream ends (a final frame without the
     *  trailing blank line). */
    flush(): SseFrame[] {
      const rest = buf;
      buf = "";
      if (!rest.trim()) return [];
      const frame = parseFrame(rest);
      return frame ? [frame] : [];
    },
  };
}

/** A tool frame → komo's `TurnEvent`, or null when it isn't one / is malformed
 *  (a bad frame must not abort the turn). */
export function toolEventFrom(frame: SseFrame): TurnEvent | null {
  if (frame.event !== "tool") return null;
  try {
    return JSON.parse(frame.data) as TurnEvent;
  } catch {
    return null;
  }
}

/** A completion chunk → its text delta, or null when there is none. */
export function textDeltaFrom(frame: SseFrame): string | null {
  if (frame.event === "tool") return null;
  try {
    const chunk = JSON.parse(frame.data) as {
      choices?: { delta?: { content?: string } }[];
    };
    return chunk?.choices?.[0]?.delta?.content ?? null;
  } catch {
    return null;
  }
}
