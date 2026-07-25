import { fileURLToPath, URL } from "node:url";

import { defineConfig } from "vitest/config";

// Only pure logic is tested (orchestration, wire parsing, data mapping), so no
// DOM environment is needed — see apps/app/README.md.
export default defineConfig({
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  test: { environment: "node", include: ["src/**/*.test.ts"] },
});
