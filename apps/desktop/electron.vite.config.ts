import { fileURLToPath, URL } from "node:url";

import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "electron-vite";

// Three-part build (main / preload / renderer), mirroring mineclaw's
// electron-client. The renderer is a plain React + Vite app; main and preload
// are bundled to CommonJS (`.cjs`) so the sandboxed preload and the Electron
// main entry load without ESM friction.
export default defineConfig({
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
    plugins: [tailwindcss(), react()],
    resolve: {
      alias: { "@": fileURLToPath(new URL("./src/renderer", import.meta.url)) },
    },
    build: {
      outDir: fileURLToPath(new URL("./dist/renderer", import.meta.url)),
      emptyOutDir: true,
    },
    server: { host: "127.0.0.1", port: 5273, strictPort: true },
  },
});
