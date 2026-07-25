// The one data-plane implementation, shared by every host. It speaks plain
// HTTP + SSE to the gateway's api channel, running in the renderer/browser
// directly (nothing here needs Node). Hosts differ only in the
// `GatewayResolver` they pass: Electron reads `~/.komo/gateway.json` over IPC,
// web derives base+key from the page location + a stored key.
//
// The bearer key therefore lives in the renderer. That is the deliberate trade
// for one client shared with the web build, where the key must reach the
// browser anyway; the renderer is sandboxed and the key is loopback-scoped.

import { TIMEOUT } from "../config";
import { createFrameSplitter, textDeltaFrom, toolEventFrom } from "./sse";
import type {
  ChatOptions,
  Gateway,
  GatewayResolver,
  KomoApiRequest,
  KomoApiResponse,
  KomoChatRequest,
  KomoChatResponse,
  KomoClient,
  KomoConnectResponse,
} from "./types";

async function fetchWithTimeout(
  url: string,
  options: RequestInit,
  timeoutMs: number,
): Promise<Response> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await fetch(url, { ...options, signal: controller.signal });
  } finally {
    clearTimeout(timer);
  }
}

async function healthOk(base: string): Promise<boolean> {
  try {
    const res = await fetchWithTimeout(`${base}/health`, {}, TIMEOUT.probe);
    return res.ok;
  } catch {
    return false;
  }
}

function errMsg(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** Make `controller` follow `external`, returning the detach function. Used
 *  instead of passing `external` to `fetch` directly, because the request also
 *  needs its own timeout — and both have to be able to abort the *stream*, not
 *  just the headers phase. */
function follow(controller: AbortController, external: AbortSignal | undefined): () => void {
  if (!external) return () => {};
  if (external.aborted) {
    controller.abort(external.reason);
    return () => {};
  }
  const onAbort = () => controller.abort(external.reason);
  external.addEventListener("abort", onAbort, { once: true });
  return () => external.removeEventListener("abort", onAbort);
}

export class HttpKomoClient implements KomoClient {
  /** The endpoint the last `connect()` bound to; api/chat use it until the next
   *  `connect()` re-resolves (so a gateway restart's new port/key is picked up
   *  on the connection poll's next tick). */
  private gateway: Gateway | null = null;

  constructor(private readonly resolve: GatewayResolver) {}

  async connect(): Promise<KomoConnectResponse> {
    const found = await this.resolve();
    if (!found) {
      this.gateway = null;
      return {
        connected: false,
        error: "未发现运行中的 komo gateway（启动 `komo gateway` 后自动连接）",
      };
    }
    if (!(await healthOk(found.base))) {
      this.gateway = null;
      return { connected: false, error: "gateway 无响应（rendezvous 可能过期）" };
    }
    this.gateway = found;
    return { connected: true, base: found.base };
  }

  async api<T = unknown>(req: KomoApiRequest): Promise<KomoApiResponse<T>> {
    if (!this.gateway) return { ok: false, status: 0, error: "未连接" };
    const { path, method = "GET", body } = req;
    try {
      const res = await fetchWithTimeout(
        `${this.gateway.base}${path}`,
        {
          method,
          headers: {
            Authorization: `Bearer ${this.gateway.key}`,
            ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
          },
          body: body !== undefined ? JSON.stringify(body) : undefined,
        },
        TIMEOUT.request,
      );
      const text = await res.text();
      const data = text ? JSON.parse(text) : null;
      if (!res.ok) {
        return { ok: false, status: res.status, error: data?.error || `HTTP ${res.status}`, data };
      }
      return { ok: true, status: res.status, data };
    } catch (err) {
      return { ok: false, status: 0, error: errMsg(err) };
    }
  }

  // One chat turn over the SSE stream. `mode` picks the loopback session
  // context: interactive (approval/clarify suspend the turn, resolved
  // out-of-band) or trusted (side-effecting tools auto-approve, like
  // `komo chat`). Tool frames fire `onToolEvent` live; text deltas accumulate.
  //
  // The controller here spans the whole call — headers *and* body — so an
  // interrupt (or the timeout) also tears down a stream that has already
  // started, which `fetchWithTimeout` could not do.
  async chat(req: KomoChatRequest, options?: ChatOptions): Promise<KomoChatResponse> {
    if (!this.gateway) return { ok: false, error: "未连接" };
    const { header, message, mode } = req;
    const headers: Record<string, string> = {
      Authorization: `Bearer ${this.gateway.key}`,
      "Content-Type": "application/json",
      "X-Komo-Session-Id": header,
      ...(mode === "trusted" ? { "X-Komo-Trusted": "1" } : { "X-Komo-Interactive": "1" }),
    };

    const controller = new AbortController();
    const unfollow = follow(controller, options?.signal);
    const timer = setTimeout(() => controller.abort(), TIMEOUT.request);
    try {
      const res = await fetch(`${this.gateway.base}/v1/chat/completions`, {
        method: "POST",
        headers,
        signal: controller.signal,
        body: JSON.stringify({
          model: "komo",
          stream: true,
          messages: [{ role: "user", content: message }],
        }),
      });
      if (!res.ok || !res.body) {
        const text = await res.text().catch(() => "");
        let msg = `HTTP ${res.status}`;
        try {
          const parsed = JSON.parse(text);
          if (parsed?.error) msg = parsed.error;
        } catch {
          /* keep the status-only message */
        }
        return { ok: false, error: msg };
      }

      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      const splitter = createFrameSplitter();
      let reply = "";
      const consume = (frames: ReturnType<typeof splitter.push>) => {
        for (const frame of frames) {
          const tool = toolEventFrom(frame);
          if (tool) {
            options?.onToolEvent?.(tool);
            continue;
          }
          reply += textDeltaFrom(frame) ?? "";
        }
      };
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        consume(splitter.push(decoder.decode(value, { stream: true })));
      }
      consume(splitter.flush());
      return { ok: true, reply };
    } catch (err) {
      return { ok: false, error: errMsg(err) };
    } finally {
      clearTimeout(timer);
      unfollow();
    }
  }
}
