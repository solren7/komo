import React from "react";
import { createRoot } from "react-dom/client";

import { HttpKomoClient, KomoApp, installHost, type Gateway } from "@komo/app";
import "./styles.css";

// Desktop gateway resolver: read ~/.komo/gateway.json over the preload bridge
// each time, so a gateway restart's new port/key is picked up on the next
// connection-poll tick. All HTTP then goes straight from here to loopback.
const resolveGateway = async (): Promise<Gateway | null> =>
  (await window.komoBridge.gateway()) ?? null;

installHost({ client: new HttpKomoClient(resolveGateway), tag: "desktop" });

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <KomoApp />
  </React.StrictMode>,
);
