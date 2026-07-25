import { beforeEach, describe, expect, it } from "vitest";

import { ApiError } from "./errors";
import { apiField, apiGet, apiPost } from "./request";
import { installClient } from "./runtime";
import type { KomoApiResponse, KomoClient } from "./types";

function fakeClient(reply: KomoApiResponse): { client: KomoClient; calls: unknown[] } {
  const calls: unknown[] = [];
  const client: KomoClient = {
    connect: async () => ({ connected: true }),
    api: async (req) => {
      calls.push(req);
      return reply as KomoApiResponse<never>;
    },
    chat: async () => ({ ok: true, reply: "" }),
  };
  return { client, calls };
}

describe("request helpers", () => {
  beforeEach(() => {
    installClient(fakeClient({ ok: true, status: 200, data: null }).client, "web");
  });

  it("returns the payload on success", async () => {
    installClient(fakeClient({ ok: true, status: 200, data: { a: 1 } }).client, "web");
    await expect(apiGet<{ a: number }>("/api/x")).resolves.toEqual({ a: 1 });
  });

  it("throws an ApiError carrying the status", async () => {
    installClient(fakeClient({ ok: false, status: 503, error: "down" }).client, "web");
    await expect(apiGet("/api/x")).rejects.toMatchObject({ message: "down", status: 503 });
    await expect(apiGet("/api/x")).rejects.toBeInstanceOf(ApiError);
  });

  it("falls back to the status when no error message is given", async () => {
    installClient(fakeClient({ ok: false, status: 500 }).client, "web");
    await expect(apiPost("/api/x")).rejects.toMatchObject({ message: "HTTP 500" });
  });

  it("unwraps an envelope field", async () => {
    installClient(fakeClient({ ok: true, status: 200, data: { sessions: [1, 2] } }).client, "web");
    await expect(apiField<number[]>("/api/sessions", "sessions")).resolves.toEqual([1, 2]);
  });

  it("unwraps an empty list without complaining", async () => {
    installClient(fakeClient({ ok: true, status: 200, data: { sessions: [] } }).client, "web");
    await expect(apiField<number[]>("/api/sessions", "sessions")).resolves.toEqual([]);
  });

  it("throws when the envelope field is missing — a renamed field must not read as empty", async () => {
    installClient(fakeClient({ ok: true, status: 200, data: { items: [] } }).client, "web");
    await expect(apiField("/api/sessions", "sessions")).rejects.toThrow(/sessions/);
  });

  it("sends the method and body through to the client", async () => {
    const fake = fakeClient({ ok: true, status: 200, data: null });
    installClient(fake.client, "web");
    await apiPost("/api/x", { y: 1 });
    expect(fake.calls).toEqual([{ path: "/api/x", method: "POST", body: { y: 1 } }]);
  });
});
