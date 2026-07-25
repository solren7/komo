import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, URL } from "node:url";

import { defineConfig } from "electron-vite";

import { sharedAlias, sharedPlugins } from "../vite.shared";

/** The gateway to proxy dev requests to: `KOMO_DEV_GATEWAY` if set, else
 *  whatever `~/.komo/gateway.json` currently advertises. Resolved once, when the
 *  dev server starts — the renderer compares it against the live rendezvous and
 *  falls back to talking to the gateway directly if it has since moved (see
 *  src/renderer/main.tsx). */
function devGateway(): string | null {
  const explicit = process.env.KOMO_DEV_GATEWAY;
  if (explicit) return explicit;
  try {
    const home = process.env.KOMO_HOME || join(homedir(), ".komo");
    const info = JSON.parse(readFileSync(join(home, "gateway.json"), "utf8"));
    const host = info.bind === "0.0.0.0" ? "127.0.0.1" : info.bind;
    return `http://${host}:${info.port}`;
  } catch {
    return null;
  }
}

// Three-part build (main / preload / renderer). Main and preload bundle to
// CommonJS (`.cjs`) so the sandboxed preload and the Electron main entry load
// without ESM friction. The renderer is a thin host that mounts @komo/app.
export default defineConfig(({ command }) => {
  // In dev the renderer is served by Vite on its own origin, so gateway
  // requests would be cross-origin. Proxy them to keep dev same-origin; the
  // gateway's CORS layer covers the packaged build (and a stale target).
  const target = command === "serve" ? devGateway() : null;
  const proxy = target
    ? Object.fromEntries(
        ["/api", "/v1", "/health"].map((path) => [path, { target, changeOrigin: true }]),
      )
    : undefined;

  return {
    main: {
      build: {
        outDir: "dist/main",
        lib: { entry: "src/main/index.ts" },
        rollupOptions: {
          external: ["electron"],
          output: { format: "cjs", entryFileNames: "[name].cjs", inlineDynamicImports: true },
        },
      },
    },
    preload: {
      build: {
        outDir: "dist/preload",
        lib: { entry: "src/preload/index.ts" },
        rollupOptions: {
          external: ["electron"],
          output: { format: "cjs", entryFileNames: "[name].cjs", inlineDynamicImports: true },
        },
      },
    },
    renderer: {
      root: fileURLToPath(new URL("./src/renderer", import.meta.url)),
      plugins: sharedPlugins(),
      resolve: { alias: sharedAlias },
      define: { __KOMO_DEV_PROXY_TARGET__: JSON.stringify(target) },
      build: {
        outDir: fileURLToPath(new URL("./dist/renderer", import.meta.url)),
        emptyOutDir: true,
      },
      server: { host: "127.0.0.1", port: 5273, strictPort: true, proxy },
    },
  };
});
