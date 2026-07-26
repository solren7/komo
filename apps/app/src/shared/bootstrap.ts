// The one call a host makes before rendering: install its `KomoClient` +
// platform tag, seed the first session id, and apply the persisted theme
// (before first paint, so there's no light→dark flash).

import { installClient } from "./api/runtime";
import type { KomoClient } from "./api/types";
import type { HostTag } from "./lib/session-id";
import { newSessionId } from "./lib/session-id";
import { applyTheme } from "./lib/theme";
import { useAppStore } from "./store";

export function installHost({ client, tag }: { client: KomoClient; tag: HostTag }): void {
  installClient(client, tag);
  const store = useAppStore.getState();
  if (!store.session) {
    const session = newSessionId(tag);
    useAppStore.setState({
      session,
      workspaceSessions: { ...store.workspaceSessions, [store.workspace]: session },
    });
  }
  applyTheme(useAppStore.getState().theme);
}
