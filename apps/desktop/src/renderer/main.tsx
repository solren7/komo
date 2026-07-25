import React from "react";
import { createRoot } from "react-dom/client";

import { HttpKomoClient, KomoApp, installHost, type Gateway } from "@komo/app";
import "./styles.css";

// Desktop gateway resolver: read ~/.komo/gateway.json over the preload bridge
// each time, so a gateway restart's new port/key is picked up on the next
// connection-poll tick.
//
// Where the requests go depends on the origin this renderer runs on. Packaged,
// it is `file://` and everything goes straight to loopback (the gateway's CORS
// layer grants the opaque `null` origin). In dev it is Vite's own origin, so
// when Vite is proxying to this exact gateway we talk to it same-origin through
// that proxy; if the gateway has since restarted on another port the proxy
// target is stale, so we go direct and rely on CORS.
const resolveGateway = async (): Promise<Gateway | null> => {
  const found = await window.komoBridge.gateway();
  if (!found) return null;
  return __KOMO_DEV_PROXY_TARGET__ === found.base ? { ...found, base: "" } : found;
};

installHost({ client: new HttpKomoClient(resolveGateway), tag: "desktop" });

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <KomoApp />
  </React.StrictMode>,
);
