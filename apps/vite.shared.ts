import { fileURLToPath, URL } from "node:url";

import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// `@komo/app` is a source-only workspace package: hosts bundle its TypeScript
// directly, so both the barrel and the app's internal `@` alias have to resolve
// to the same directory in every host's build.
const appSrc = fileURLToPath(new URL("./app/src", import.meta.url));

export const sharedPlugins = () => [tailwindcss(), react()];

export const sharedAlias = {
  "@komo/app": appSrc,
  "@": appSrc,
};
