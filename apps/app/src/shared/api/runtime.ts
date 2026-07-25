// The active `KomoClient` and host tag, installed once by `installHost()`
// before the app renders. A module singleton mirrors the shape of the thing it
// models: there is exactly one gateway connection per window, so threading it
// through every component would be ceremony.

import type { HostTag } from "../lib/session-id";
import type { KomoClient } from "./types";

let client: KomoClient | null = null;
let tag: HostTag = "web";

export function installClient(next: KomoClient, host: HostTag): void {
  client = next;
  tag = host;
}

export function getClient(): KomoClient {
  if (!client) throw new Error("KomoClient not installed — call installHost() before render");
  return client;
}

export function hostTag(): HostTag {
  return tag;
}
