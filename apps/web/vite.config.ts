import { defineConfig, loadEnv } from "vite";

import { sharedAlias, sharedPlugins } from "../vite.shared";

// In production the gateway serves this build same-origin, so requests to
// /api, /v1, /health hit the same host with no CORS. In dev, set
// KOMO_DEV_GATEWAY=http://127.0.0.1:<port> (a gateway with `[channels.api]`
// bound to a fixed port) and Vite proxies those paths there, so the browser
// stays same-origin against the dev server.
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "KOMO_");
  const target = env.KOMO_DEV_GATEWAY;
  const proxy = target
    ? Object.fromEntries(
        ["/api", "/v1", "/health"].map((path) => [path, { target, changeOrigin: true }]),
      )
    : undefined;
  return {
    plugins: sharedPlugins(),
    resolve: { alias: sharedAlias },
    server: { host: "127.0.0.1", port: 5274, strictPort: true, proxy },
    build: { outDir: "dist", emptyOutDir: true },
  };
});
