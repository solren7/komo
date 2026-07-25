import React, { useState } from "react";
import { createRoot } from "react-dom/client";

import {
  ConnectGate,
  HttpKomoClient,
  KomoApp,
  consumeQueryParams,
  currentGateway,
  installHost,
} from "@komo/app";
import "./styles.css";

// Same-origin by default (the gateway serves this build); the bearer key comes
// from a `?key=` param on first load or from the connect screen.
consumeQueryParams();
installHost({
  client: new HttpKomoClient(async () => currentGateway()),
  tag: "web",
});

function Root() {
  const [ready, setReady] = useState(() => currentGateway() !== null);
  if (!ready) return <ConnectGate onSaved={() => setReady(true)} />;
  return <KomoApp />;
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
