# Running shion on TrueNAS SCALE (Docker)

shion's gateway is a good fit for a NAS: it makes **only outbound connections**
(Telegram long-poll, Feishu WebSocket, the LLM API, and your LAN Home Assistant),
so there are **no inbound ports to publish or forward**. It runs entirely in a
container; the only host resource it needs is one dataset for durable state.

This guide targets **TrueNAS SCALE** (Linux, native Docker in Electric Eel
24.10+). TrueNAS **CORE** (FreeBSD) has no Docker — there you'd use a jail or a
Linux VM instead.

## 1. Build and push the image

shion builds on Linux unchanged (the macOS notifier degrades to a no-op; the
launchd installer is `cfg`-gated off). The repo ships a multi-stage `Dockerfile`.

### Option A — GitHub Actions (recommended)

`.github/workflows/docker.yml` builds the image on GitHub's **native amd64**
runners and pushes it to GHCR. No local Docker, no QEMU cross-build. It runs on
every push to `main` (and on `v*` tags, and via the Actions tab → "Run
workflow"). The image lands at:

```
ghcr.io/solren7/shion:latest
```

**Make the package pullable by the NAS.** GHCR packages are **private** by
default. Either:

- Open the package (GitHub → your profile → Packages → `shion` → Package
  settings → Change visibility → Public), then the NAS pulls anonymously; **or**
- Keep it private and log the NAS in with a Personal Access Token that has
  `read:packages`:
  ```bash
  docker login ghcr.io -u solren7 -p <PAT_with_read:packages>
  ```

### Option B — build locally with buildx

> **Architecture matters.** TrueNAS is almost always `amd64`. On an Apple
> Silicon Mac, a plain `docker build` produces an `arm64` image that **won't run
> on the NAS**. Use `buildx` with an explicit platform:

```bash
docker buildx build \
  --platform linux/amd64 \
  -t ghcr.io/solren7/shion:latest \
  --push .
```

(If your NAS is ARM, use `--platform linux/arm64`. To pin a Rust version for
reproducible builds, edit the `FROM rust:1-bookworm` tag in the `Dockerfile`.)

## 2. Prepare the dataset (and migrate your existing setup)

Create a dataset for shion's state, e.g. `apps/shion` on your pool. It will hold
`config.toml`, `.env`, the three SQLite dbs (`shion.db`, `kanban.db`,
`memory.db`), and `logs/`.

If you already run shion on your Mac, the simplest migration is to copy your
existing `~/.shion` onto the dataset — SQLite files are cross-platform, so your
memories, tasks, pairings, and config all carry over:

```bash
# from the Mac, into the TrueNAS dataset (adjust host/path)
rsync -av ~/.shion/ root@truenas:/mnt/<pool>/apps/shion/
```

Otherwise just place a `config.toml` and `.env` in the dataset. Minimum `.env`:

```bash
DEEPSEEK_API_KEY=sk-...          # or your chosen provider's key
TELEGRAM_BOT_TOKEN=...           # if using the telegram channel
HASS_TOKEN=...                   # if using Home Assistant
HASS_URL=http://192.168.1.100:8123
```

## 3. Deploy

Edit `docker-compose.yml`: set the `image:` to what you pushed and the volume's
host path to your dataset (`/mnt/<pool>/apps/shion`). Then either:

- **TrueNAS UI**: Apps → Custom App → install via YAML, paste the compose, or
- **Shell**: `docker compose up -d`

Set `TZ` to your timezone (the container is UTC by default) — reminders, the
task-due sweep, and the daily briefing all fire on **local** time.

## 4. Day-2 operations

The container's entrypoint is `shion`, so the operator CLI subcommands run via
`docker exec` (which bypasses the entrypoint):

```bash
docker exec shion shion pair list            # see pending pairings
docker exec shion shion pair approve <code>  # admit a new chat sender
docker exec shion shion memory list
docker exec shion shion task list
docker exec shion shion run list
```

Logs: `docker logs -f shion` (the gateway logs to stderr). Bump verbosity with
`SHION_LOG=debug` in the compose `environment:`.

Update to a new build: rebuild + push (step 1), then re-pull and recreate:

```bash
docker compose pull && docker compose up -d
```

State on the dataset is untouched by redeploys.

## 5. WeChat (微信) channel — QR login in a headless container

The WeChat channel logs in over the iLink protocol by **scanning a QR code**,
which the always-on gateway can't render. So provisioning is a one-time
interactive step; after it, the gateway reuses the stored credentials.

1. **Enable it** in the dataset's `config.toml`:

   ```toml
   [channels.wechat]
   enabled = true
   allow_from = ["o9cq...@im.wechat"]   # your iLink user id (skip pairing); optional
   home_chat = "o9cq...@im.wechat"       # optional: send reminders/briefing here
   ```

   Deploy/restart once. The channel starts **inert** and logs `no stored
   credentials` — that's expected; the rest of the gateway runs normally.

2. **Provision credentials.** The cleanest way needs **no shell on the NAS** —
   drive it from an existing chat channel:

   - **From Telegram (recommended).** With the `telegram` channel already
     working, send the bot **`/wechat login`**. The gateway replies with the
     login QR **as a photo**; scan it with the WeChat app and confirm. Creds are
     written to `/data/wechat/credentials.json`, and the WeChat channel **comes
     online immediately — no restart**. (The channel waits for credentials, so
     it's fine that it booted without them.)
   - **Scan inside the container.** `docker exec -it` gives a TTY, so the QR
     renders in your shell:
     ```bash
     docker exec -it shion shion wechat login
     ```
   - **Or reuse a desktop login**: run `shion wechat login` on a machine with a
     screen, then copy its `~/.shion/wechat/credentials.json` to
     `/mnt/<pool>/apps/shion/wechat/credentials.json` on the NAS and restart.

   `docker logs shion` (or `shion logs`) should show `wechat channel connected`.

> **Run the gateway in exactly one place.** WeChat (and Telegram) authenticate
> as a single identity; if both your Mac and the NAS run the channel against the
> same account/token, their long-polls fight (`Conflict: terminated by other
> getUpdates request`). Pick the always-on host (the NAS) and disable the
> channel elsewhere.

## Gotchas recap

- **Build arch** — cross-build for the NAS (`--platform`), don't ship your
  laptop's arch.
- **Timezone** — set `TZ`, or schedules drift by your UTC offset.
- **Persistence** — everything lives under the mounted `/data`; an unmounted
  container loses all state on recreate.
- **Notifications** — the macOS popup notifier no-ops on Linux. Set a channel
  `home_chat` (Telegram/Feishu) so reminders/briefings have somewhere to go.
- **Home Assistant on the LAN** — the default bridge network can reach LAN IPs,
  so `HASS_URL=http://192.168.x.x:8123` works. If HA is a container on the same
  host, use the host's LAN IP (not `localhost`).
- **No launchd** — don't use `shion gateway start/stop` in the container; the
  compose `restart: unless-stopped` policy is the supervisor.
- **One gateway per identity** — WeChat/Telegram poll as a single account; don't
  run the same channel on the NAS *and* your Mac, or their long-polls conflict.
- **WeChat QR** — login is interactive; provision creds once with
  `docker exec -it shion shion wechat login`, then restart (see §5).
