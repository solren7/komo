# komo — Electron desktop shell

A thin Electron host over the shared [`@komo/app`](../app) renderer. The same
React app runs here and in the standalone [web build](../web); this package only
adds a native window and gateway discovery. All UI conventions (layout, theme,
state, tests) are documented in [`../app/README.md`](../app/README.md).

## What it does

- **Auto-discovers** a running gateway via `~/.komo/gateway.json` (read in the
  Electron main process, re-read on each connection tick so a restart's new
  port/key is picked up).
- Everything else — chat (`@assistant-ui/react`), interactive tool
  approval + clarify (polling `/api/interactions/{session}`), the
  status/tasks/memories/runs dashboard, session rename/archive/delete — lives in
  `@komo/app` and is shared with the web build. See [../app](../app).
- Sessions started here are tagged `api:gui-desktop-<uuid>`, so the session list
  shows which client opened them.

## Architecture

The renderer is platform-agnostic and talks to the gateway only through a
`KomoClient` (see `@komo/app`'s `shared/api/`). This shell wires up the one HTTP
implementation with a desktop-specific gateway resolver:

- **Main** (`src/main/index.ts`): gateway discovery only. Reads
  `~/.komo/gateway.json` and returns `{base, key}` over a single IPC channel
  (`komo:gateway`). No HTTP proxying — the renderer calls the gateway directly.
- **Preload** (`src/preload/index.ts`): the only bridge — `window.komoBridge.gateway()`.
- **Renderer** (`src/renderer/main.tsx`): builds `new HttpKomoClient(resolver)`
  over that bridge, hands it to `installHost({ client, tag: "desktop" })`, and
  mounts `<KomoApp/>` from `@komo/app`. `src/renderer/styles.css` imports the
  shared stylesheet and points Tailwind at this host's source. The renderer
  stays sandboxed (`contextIsolation`, no node integration).

Because the renderer does its own HTTP, **the page origin matters**: packaged,
it is `file://` (origin `null`) and requests go straight to loopback, which the
gateway's CORS layer grants; in dev the renderer is served by Vite on
`127.0.0.1:5273`, so `electron.vite.config.ts` proxies `/api`, `/v1` and
`/health` to whatever `~/.komo/gateway.json` advertised when the dev server
started, and the renderer talks same-origin through that proxy. If the gateway
restarts on a new port mid-session the proxy target goes stale, so the renderer
compares it against the live rendezvous and goes direct (via CORS) when they
differ — no dev-server restart needed.

Unlike the earlier REST-over-IPC design, the bearer key now lives in the
renderer — the deliberate trade for sharing one client with the web build (where
the key must reach the browser regardless). The renderer is sandboxed and the
key is loopback/key-scoped on the gateway side.

Single request/response per turn — komo streams tool-call events over SSE but
not token deltas, so a turn suspends server-side for approval/clarify and the
same HTTP request returns the final reply.

## Run

Install once at the workspace root (`apps/`), then start a komo gateway (so
`~/.komo/gateway.json` exists), then:

```bash
cd apps && bun install
```

```bash
cd apps/desktop && bun run dev
```

`bun run build && bun run start` does a production build then launches.

## Known limitations (demo scope)

- No token streaming (spinner + whole reply), mirroring the backend.
- "Continue in chat" resumes the **server-side** session context (history
  threads correctly); past messages are re-hydrated from the run ledger.
- Not packaged (`electron-builder`); `dev` / `start` only.
