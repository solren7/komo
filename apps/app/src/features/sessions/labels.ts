// Session ids are opaque; this is the fallback label when a session has no
// user-set title. The `gui-<host>-` prefix is our own convention (see
// shared/lib/session-id.ts) — `electron` is the pre-rename form and still
// appears in existing sessions, so it stays recognised.

const HOST_LABEL: Record<string, string> = {
  desktop: "桌面",
  electron: "桌面",
  web: "浏览器",
};

const GUI_ID = /^gui-(desktop|electron|web)-.*?([0-9a-f]{4,})$/i;

export function sessionLabel(id: string): string {
  const bare = id.replace(/^api:/, "");
  const match = bare.match(GUI_ID);
  if (match) {
    const host = HOST_LABEL[match[1].toLowerCase()] ?? "";
    return `${host}会话 ${match[2].slice(-6)}`;
  }
  return bare.length > 22 ? `${bare.slice(0, 20)}…` : bare;
}
