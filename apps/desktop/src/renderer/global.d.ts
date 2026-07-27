// The preload bridge exposed on `window.komoBridge`: gateway discovery plus the
// native directory dialog — the renderer's HttpKomoClient (from @komo/app) does
// all HTTP itself.

import type { Gateway } from "@komo/app";

declare global {
  interface Window {
    komoBridge: {
      /** The current gateway endpoint, or null when none is running. */
      gateway(): Promise<Gateway | null>;
      /** Open the OS directory dialog; null when the operator cancels. */
      chooseWorkspace(): Promise<{ name: string; path: string } | null>;
    };
  }

  /** Injected by electron.vite.config.ts: the gateway the dev server proxies
   *  to, or null (production build, or no gateway found at dev-server start). */
  const __KOMO_DEV_PROXY_TARGET__: string | null;
}

export {};
