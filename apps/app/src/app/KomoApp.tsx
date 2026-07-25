import { App } from "./App";
import { AppProviders } from "./providers";

/** What a host mounts: the app plus every provider it needs. */
export function KomoApp() {
  return (
    <AppProviders>
      <App />
    </AppProviders>
  );
}
