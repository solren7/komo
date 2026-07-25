// Session ids are client-generated. Only the `api:` prefix has meaning to the
// gateway (it re-prepends it from the `X-Komo-Session-Id` header); the
// `gui-<host>-` part is a local convention that keeps desktop and browser
// sessions distinguishable in the list.

/** Which shell is running the renderer. */
export type HostTag = "desktop" | "web";

const API_PREFIX = "api:";

export function newSessionId(host: HostTag): string {
  return `${API_PREFIX}gui-${host}-${crypto.randomUUID()}`;
}

/** Full session id → the `X-Komo-Session-Id` header value. */
export function headerFor(fullSession: string): string {
  return fullSession.startsWith(API_PREFIX) ? fullSession.slice(API_PREFIX.length) : fullSession;
}
