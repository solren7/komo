// The client seam. The renderer is platform-agnostic: it reaches the gateway
// only through a `KomoClient`. Each host constructs one over HTTP and installs
// it via `runtime.ts` — the desktop shell only *resolves* the gateway
// address/key (over its preload bridge) and hands it to the same
// `HttpKomoClient` the web build uses.

import type { Mode } from "../types";

/** A resolved gateway endpoint: base URL + bearer key. */
export interface Gateway {
  base: string;
  key: string;
}

/** Yields the current gateway endpoint, or null when none is reachable/known.
 *  Desktop reads `~/.komo/gateway.json` (over IPC, re-read each call so a
 *  restart's new port/key is picked up); web derives it from the location +
 *  a stored key. */
export type GatewayResolver = () => Promise<Gateway | null>;

export interface KomoApiRequest {
  path: string;
  method?: "GET" | "POST";
  body?: unknown;
}

export interface KomoApiResponse<T = unknown> {
  ok: boolean;
  status: number;
  data?: T;
  error?: string;
}

export interface KomoChatRequest {
  header: string;
  message: string;
  mode: Mode;
}

export interface KomoChatResponse {
  ok: boolean;
  reply?: string;
  error?: string;
}

export interface KomoConnectResponse {
  connected: boolean;
  base?: string;
  error?: string;
}

/** A live tool-call event streamed during a turn (mirrors komo's `TurnEvent`). */
export type TurnEvent =
  | { type: "tool_started"; seq: number; name: string; args: string }
  | { type: "tool_finished"; seq: number; name: string; ok: boolean; summary: string };

export interface ChatOptions {
  /** Fires per live `event: tool` frame while the turn runs. */
  onToolEvent?: (event: TurnEvent) => void;
  /** Aborts the request — including mid-stream — when the user interrupts. */
  signal?: AbortSignal;
}

/** The renderer's entire data plane. One HTTP implementation (`client.ts`)
 *  backs every host. */
export interface KomoClient {
  /** Probe reachability and (re)bind to the current gateway endpoint. */
  connect(): Promise<KomoConnectResponse>;
  /** One authenticated `/api/*` or `/v1/*` request. */
  api<T = unknown>(req: KomoApiRequest): Promise<KomoApiResponse<T>>;
  /** One chat turn over the SSE stream. Tool frames fire `onToolEvent` live and
   *  the final assistant text is returned; `signal` interrupts the turn. */
  chat(req: KomoChatRequest, options?: ChatOptions): Promise<KomoChatResponse>;
}
