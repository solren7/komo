import React from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";

import { App } from "./App";
import { queryClient } from "./lib/query-client";
import { applyTheme, initialTheme } from "./lib/theme";
import "./styles/main.css";

// Apply the persisted theme before the first paint to avoid a light→dark flash.
applyTheme(initialTheme());

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </React.StrictMode>,
);
