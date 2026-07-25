// Where the browser build keeps its gateway endpoint between visits. The key
// must reach the browser (there is no main process to hold it, unlike the
// desktop shell); the gateway's api channel authenticates it and is
// loopback/key-scoped.

import type { Gateway } from "@/shared/api/types";

const KEY_STORE = "komo.key";
const BASE_STORE = "komo.base";

/** Pull `?key=`/`?token=` and `?base=` into localStorage on first load, then
 *  strip them from the address bar so the key isn't left in history. */
export function consumeQueryParams(): void {
  const url = new URL(location.href);
  let changed = false;
  const key = url.searchParams.get("key") ?? url.searchParams.get("token");
  if (key) {
    localStorage.setItem(KEY_STORE, key);
    url.searchParams.delete("key");
    url.searchParams.delete("token");
    changed = true;
  }
  const base = url.searchParams.get("base");
  if (base) {
    localStorage.setItem(BASE_STORE, base);
    url.searchParams.delete("base");
    changed = true;
  }
  if (changed) history.replaceState(null, "", url.toString());
}

/** The web `GatewayResolver`: same-origin by default (the gateway serves this
 *  build), overridable via a stored base; null until a key is known. */
export function currentGateway(): Gateway | null {
  const key = localStorage.getItem(KEY_STORE);
  if (!key) return null;
  const base = localStorage.getItem(BASE_STORE) || location.origin;
  return { base, key };
}

export function storeGateway({ base, key }: { base: string; key: string }): void {
  localStorage.setItem(KEY_STORE, key);
  if (base) localStorage.setItem(BASE_STORE, base);
  else localStorage.removeItem(BASE_STORE);
}

export function storedBase(): string {
  return localStorage.getItem(BASE_STORE) ?? "";
}
