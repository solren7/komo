# AGENTS.md

Guidance for coding agents (Claude Code and others) working in this repository.
`CLAUDE.md` is a symlink to this file — edit `AGENTS.md` only.

## Commands

```bash
cargo check                        # fast compile check
cargo build                        # build
cargo run -- chat                  # start interactive chat (db lives at ~/.shion/shion.db)
cargo run -- gateway               # always-on process: maintenance sweeps + ingress channels (feishu, telegram, wechat)
cargo test                         # run all tests
cargo test tools::time             # run a single test module
cargo fmt                          # format

shion gateway start                # install + start under launchd (auto-restart, login start)
shion gateway stop                 # stop and remove from launchd
shion gateway restart              # regenerate plist + restart (picks up a reinstalled binary)
shion gateway status               # launchd state (state/pid/last exit code)
shion logs [-n N] [-f] [--stdout]  # tail the gateway tracing log (-f follows; --stdout shows gateway.log)

shion memory list [--status S]     # list/triage memories (candidate/active/archived/rejected)
shion memory search <query>        # substring search across all memories
shion memory promote <id>          # candidate → active+confirmed
shion memory reject <id>           # candidate → rejected
shion memory pin <id>              # pin into the L1 per-turn profile (manual-only path)

shion run list [--limit N]         # recent runs (one per turn), newest first
shion run inspect <id>             # one run in full: input, plan, outcome, every tool step

shion wechat login                 # provision WeChat iLink creds by scanning a QR (run on the host)
```

Logs: a `tracing` subscriber is installed in `main.rs` (`init_tracing`) — without
it every `info!`/`warn!`/`debug!` is a silent no-op. Output goes to stderr
(launchd captures the gateway's via the plist's `StandardErrorPath` →
`~/.shion/logs/gateway.err.log`). Level is `SHION_LOG` (e.g. `SHION_LOG=debug`),
defaulting to `info,toasty=warn,rig_core=warn` (shion's own logs at info; ORM
schema chatter muted; and rig's `prompt_request` INFO events muted — they log
every tool call's *full result* verbatim, a wall of text for any list-returning
tool). Each turn runs inside a `run` span (`run_id`) and each tool call inside a
`tool` span (`name`/`seq`) and is recorded by shion's own concise `tool ok`
line (name/seq/elapsed, no result), so live logs still line up with the
persisted run ledger. Set `SHION_LOG=debug` (or `rig_core=info`) to see the full
tool results again.

`~/.shion/shion.db` is disposable developer state (sessions, messages, session
todos, skills, reminders, pairings, settings, **run ledger**) — delete it freely
to reset.
Two kinds of **durable personal data live in their own files** so resetting
`shion.db` never wipes them: cross-session **tasks in `~/.shion/kanban.db`**
(`infra/persistence/kanban.rs`) and long-term **memories in `~/.shion/memory.db`**
(`infra/memory/memory_db.rs`). After a schema change, delete the affected file —
`push_schema` only runs for newly created database files: a `TaskRecord` change
means deleting `kanban.db`, a `MemoryRecord` change means `memory.db`, any other
model means `shion.db` (e.g. a `RunRecord`/`RunStepRecord` change — the run
ledger lives in `shion.db`).

Building requires `protoc` (`brew install protobuf`): the feishu channel's websocket
frames are protobuf, and `lark-websocket-protobuf` compiles its `.proto` at build time.

Runtime settings (provider/model/base_url/aux_model, maintenance `schedule`,
the opt-in daily `briefing_schedule`, the `[channels.*]` tables) live in
`~/.shion/config.toml`; credentials (API keys,
`FEISHU_APP_ID` / `FEISHU_APP_SECRET`, `TELEGRAM_BOT_TOKEN`, `HASS_TOKEN`) only
in `~/.shion/.env`. Priority: built-in defaults < config.toml < `SHION_*` env
vars. `SHION_HOME` relocates the whole directory.

Home Assistant keeps its URL and token in `.env` as a single self-contained
block: `HASS_TOKEN` (required — a long-lived access token) and `HASS_URL`
(optional, defaults to `http://homeassistant.local:8123`). These are shared by
both HA surfaces. No token = neither the `homeassistant` tool nor the channel
loads.

```bash
# ~/.shion/.env
HASS_TOKEN=your-long-lived-access-token
HASS_URL=http://192.168.1.100:8123   # optional; omit for homeassistant.local:8123
```

The `homeassistant` **tool** (agent controls HA) registers automatically once
`HASS_TOKEN` is set — no config.toml needed. The HA **event channel** (HA
pushes device events to the agent) is opt-in via `[channels.homeassistant]`,
which carries only event-filter behavior (URL/token still come from `.env`).
Forwarding is closed by default — set at least one of `watch_domains` /
`watch_entities` / `watch_all`:

```toml
[channels.homeassistant]
enabled = true
watch_domains = ["binary_sensor", "lock", "alarm_control_panel"]
watch_entities = ["cover.garage_door"]
ignore_entities = ["binary_sensor.always_chatty"]
watch_all = false            # forward every entity (overrides the watch lists)
cooldown_seconds = 30        # per-entity min seconds between forwarded events
```

Env management: dotenvy loads `.env` files into the process env (`main.rs`); envy
deserializes them into typed structs in `config.rs` (`ShionEnv` for `SHION_*`,
`ApiKeys` for provider keys, `FeishuEnv` for `FEISHU_*`, `TelegramEnv` for
`TELEGRAM_*`). Read env vars through those structs, not `std::env::var` — the
only exception is `SHION_HOME`, the bootstrap variable that locates `.env` itself.

Channel declarations follow hermes-agent's per-platform block shape — behavior
keys in the table, credentials in env:

```toml
[channels.feishu]
enabled = true
allow_from = ["ou_xxx"]   # pre-trusted sender open_ids (skip pairing)
require_mention = true     # group messages must carry an @mention (DMs bypass)
home_chat = "oc_xxx"      # optional: reminders go here instead of macOS notifications

[channels.telegram]
enabled = true
allow_from = ["123456789"]  # pre-trusted sender user-ids (skip pairing)
allowed_chats = ["-100123"]  # group chat-id allowlist (empty = any group; DMs always pass)
require_mention = true       # group messages must @mention the bot (DMs bypass)
home_chat = "123456789"     # optional: reminders go here instead of macOS notifications

[channels.wechat]
enabled = true
allow_from = ["wxid_xxx"]   # pre-trusted iLink user-ids (skip pairing)
home_chat = "wxid_xxx"      # optional: reminders go here instead of macOS notifications
```

WeChat (微信) has no credentials in config.toml or `.env`: login is QR-based and
the iLink token is stored in `~/.shion/wechat/credentials.json`. Provision it
once on the host with `shion wechat login` (scan the QR with the WeChat app); the
gateway can't render a QR, so its `[channels.wechat]` is **inert until those
credentials exist**. WeChat is DM-only (an iLink bot identity can't join ordinary
groups), so there is no `require_mention`/`allowed_chats` — pairing is the only
admission control. Proactive output (reminders/briefing) reaches a WeChat user
only after they've messaged the bot since the gateway started — see the channel
note below.

When multiple channels set `home_chat`, feishu takes reminder delivery. The
config `home_chat` is only a fallback: the `/sethome` chat command sets the home
channel at runtime (persisted in the db), and that override wins. See the
`HomeNotifier` in the gateway section below.

Senders outside `allow_from` must pair before the agent talks to them: their
first message gets a pairing code as the only reply, and someone with shell
access to the host runs `shion pair approve <code>`. Pairing is hardened after
hermes' `pairing.py` (`domain/pairing.rs`): the code is stored only as a salted
SHA-256 hash (never plaintext, so `shion pair list` shows pending/approved but
not the code — get it from the sender), a sender is issued at most one fresh
code per 10 min (`PAIRING_RATE_LIMIT_SECS`; codes still expire after 1h), at
most 3 senders may await approval per platform (`MAX_PENDING_PER_PLATFORM`), and
the approve path locks for 1h after 5 wrong codes (`APPROVE_MAX_FAILURES`).
`shion pair revoke <id>` un-pairs. Approval is written to the shared db, so it
takes effect on the sender's next message without a gateway restart.

## Architecture

Personal Agent framework v0.1, implemented in Rust. The codebase follows a DDD-style layered architecture.

**Request flow:**
```
CLI/channel → AgentRuntime → LlmClient (rig agent loop) → RigTool → execute_isolated → tool
                          ↘ MessageRepository · RunRepository (ledger) → Response
```
The LLM owns tool dispatch: rig's function-calling loop (`agent.prompt().max_turns()`)
decides and runs tool calls; `AgentRuntime` just persists messages, opens/closes the
run-ledger entry, and returns the reply. There is no separate planner.

**Layers and their responsibilities:**

`domain/` — pure interfaces, no I/O, no external crates
- `repository.rs` — `SessionRepository` (find/save) and `MessageRepository` (list_by_session/save); the two traits `AgentRuntime` depends on
- `tool.rs` — `Tool` trait (name / description / execute / optional `redact_args`)
- `message.rs`, `session.rs` — core value types

`infra/` is layered by concern: `infra/messaging/` (ingress channels, outbound
senders, proactive notifiers), `infra/memory/` (the memory.db connection +
legacy markdown store), `infra/persistence/` (the toasty-backed shion.db /
kanban.db connections), and two cross-cutting files at the top level —
`infra/llm.rs` (LLM backend) and `infra/rig_tool.rs` (the Tool→rig adapter).

`infra/persistence/db.rs` + `infra/persistence/kanban.rs` + `infra/memory/memory_db.rs` — the only places toasty (SQLite ORM) appears
- `Db` (`infra/persistence/db.rs`) wraps `Arc<Mutex<toasty::Db>>` over `shion.db`; implements every repository trait *except* `TaskRepository`/`MemoryRepository` (sessions, messages, skills, reminders, session todos, pairings, settings, the **run ledger** `RunRepository`)
- `KanbanDb` (`infra/persistence/kanban.rs`) is a second, independent connection over `kanban.db`; it holds only `TaskRecord` and implements `TaskRepository`. Separate file = durable tasks survive a `shion.db` reset
- `MemoryDb` (`infra/memory/memory_db.rs`) is a third, independent connection over `memory.db`; it holds only `MemoryRecord` and implements `MemoryRepository`. On first run it seeds itself from legacy `~/.shion/memory/*.md` via `import_legacy_markdown` (no-op once populated)
- all: `connect(url)` checks if the db file exists; calls `push_schema()` only for new databases (toasty's `push_schema` is not idempotent — no `IF NOT EXISTS`; toasty 0.7 has a migration engine internally but `Db` exposes no public "migrate" entry point, so adding a table to an existing file means deleting it to rebuild)
- the `Arc<Mutex<toasty::Db>>` is required, not incidental: toasty's `exec` takes `&mut self` (no internal concurrency for statement exec), so every repository call serializes through this one lock. Concurrency would need per-op `Connection` checkout from toasty's pool — a larger refactor, not done
- toasty model structs are private to their file
- SQLite URL format: `sqlite:./path.db` (single colon, not `sqlite://`)

`agent/runtime.rs` — application logic
- `AgentRuntime` holds `Arc<dyn LlmClient>` + `Arc<dyn SessionRepository>` + `Arc<dyn MessageRepository>` + `Arc<dyn RunRepository>` — no knowledge of toasty, and (since the planner was removed) no tool routing of its own
- `handle_input` owns the session lifecycle: load-or-create, append the user message, call the LLM (which drives any tool calls via rig), persist the reply
- `run_turn` wraps each turn in one ledger `Run` (open → set `RunContext` task-local + a `run` tracing span around `turn_body` → finalize with status/output/error). All ledger writes are best-effort (logged, never change the turn result). `Run.plan` is a post-hoc summary derived from the recorded step count ("respond" or "<n> tool call(s)")

`domain/llm.rs` — `LlmClient` trait (`complete(&Session) -> String`); the single seam `AgentRuntime` calls to produce a reply

`infra/llm.rs` — `RigLlm<M>`: `LlmClient` backed by the `rig` framework (`rig-core`, aliased as `rig`)
- `build_llm` constructs it for the configured provider (deepseek/openai/anthropic/openrouter), exposing the tool catalog via function calling; rig's agent loop (`agent.prompt().max_turns()`) owns multi-step tool dispatch
- sends the full session history: prior turns go through `with_history`, the latest user message is the prompt; rebuilds the tiered system prompt per turn and injects L1 pinned + L3 recalled memories (main agent only)

`services/tool_registry.rs` — `ToolRegistry` is a `HashMap<String, Arc<dyn Tool>>` catalog (`register` / `tools`); the LLM owns dispatch, so there is no keyword-routed execute path
- `execute_isolated` is the single choke point all tool calls funnel through (the LLM function-calling adapter `infra/rig_tool.rs::RigTool::call`). It runs each tool on its own panic-catching task, and — when a `RunContext` is in scope (`current_run`) — records the call as a `RunStep` (best-effort, args via `Tool::redact_args`) and wraps it in a `tool` tracing span. This is why the ledger sees **every** tool call
- it also applies one global backstop on the LLM-facing result: `cap_tool_result` truncates any Ok result over the configured byte cap at a UTF-8 boundary and appends a "narrow your query" marker — applied *after* the ledger records the original, so the audit trail stays full while the model's context stays bounded. The cap is `max_tool_result_bytes` (`SHION_MAX_TOOL_RESULT_BYTES` env > config.toml > `DEFAULT_MAX_TOOL_RESULT_BYTES` = 16 KB), resolved at startup and installed via `set_tool_result_cap` (a `OnceLock`, since rig's `ToolDyn::call` signature can't take a parameter). Sized above the per-tool self-caps (`web_fetch`/`homeassistant` trim to 8 KB) so it only catches tools that don't self-trim; deterministic truncation, not LLM summarization. A tool that wants tighter or smarter trimming still does it itself
- the `SessionContext` (`SESSION`) and `RunContext` (`RUN`) task-locals both ride here because rig's `ToolDyn::call` signature is fixed; `execute_isolated` re-establishes `SESSION` across its `spawn` and instruments the spawn with the tool span

`tools/time.rs` — first built-in tool; returns RFC 3339 UTC timestamp

`tools/homeassistant.rs` — `HomeAssistantTool`, the Home Assistant integration (reaches a smart-home instance over its REST API, 15s timeout). Four actions: `list_entities` (read; optional `domain` prefix + `area` filter), `get_state` (read one entity), and `list_services` (discover callable services per domain) are read-only; `call_service` (turn devices on/off, etc.) is side-effecting → gated through the shared `Approver` as `Risk::Normal` with a `homeassistant:{domain}.{service}` scope key (approve-for-session). Two safety floors *below* approval (HA has no service-level access control of its own): `domain`/`service`/`entity_id` are shape-validated (`valid_name` / `valid_entity_id`) to block path-traversal/SSRF in the request path, and a `BLOCKED_DOMAINS` list (`shell_command`, `command_line`, `python_script`, `pyscript`, `hassio`, `rest_command`) is refused outright — no approval unlocks it, like shell's hardline list. Registered only when `HASS_TOKEN` is set (`HASS_URL` optional, defaults to homeassistant.local:8123; resolved by `config::homeassistant_config`, wired in `cli/wiring.rs`)

`infra/messaging/homeassistant.rs` — `HomeAssistantChannel`, HA as an event-ingress channel (`Channel`, like telegram/feishu but event-driven, not conversational). Opens HA's WebSocket API (`/api/websocket`), authenticates with `HASS_TOKEN`, subscribes to `state_changed`; each qualifying event is formatted into a human-readable line (domain-aware: climate/sensor/binary_sensor/light/switch/fan/lock/alarm) and dispatched as one turn under session `homeassistant:events`, with the reply delivered back as an HA persistent notification (`HomeAssistantSender`, also a `TextSender`). Event forwarding is **closed by default** (`Filters`): no `watch_domains`/`watch_entities` + `watch_all=false` ⇒ everything dropped; an `ignore_entities` list and a per-entity `cooldown_seconds` (default 30) cap the rate so a busy home doesn't fire an LLM call per sensor tick. Auto-reconnects with `[5,10,30,60]`s backoff. **No pairing** — it's a trusted local integration keyed by `HASS_TOKEN`, not a chat with arbitrary senders. Declared in `[channels.homeassistant]` (behavior only; URL/token shared with the tool), resolved by `config::homeassistant_channel_config`, wired in `cli/gateway.rs`. Approval-requiring tool calls during an HA-triggered turn are denied (no human at the keyboard), so HA events can read/notify but not perform `Risk::Normal` actions unattended.

`domain/task.rs` + `tools/task.rs` — durable cross-session tasks (roadmap §2's "kanban layer", shaped after hermes-agent), persisted by `KanbanDb` in its own `kanban.db`
- single `Task` model: `status` (`inbox`→`todo`→`done`, plus `waiting`/`cancelled`), `waiting_on` (set = a commitment), optional `due_at`, `source`/`source_message_id` (origin session + dedup key for reviewer commitment extraction, see `ReviewSweep`), `board` (optional project/grouping label — a plain string, not a Project entity; the §2 escape hatch, as hermes does)
- `task` tool actions: `capture` (defaults to inbox) / `list` (filter by `status` and/or `board`) / `update` / `complete`; no `plan_today` — daily planning belongs to the briefing sweep
- operator view: `shion task list` (open tasks grouped by status, board shown as `#board`)
- deliberately NOT modeled: task-to-task dependency edges (`blockedBy`/`blocks`) or `owner` — those serve autonomous worker-swarm orchestration (hermes kanban's `task_links`, Claude Code's Task\* tools), which shion (single-turn personal assistant, no dispatcher) does not have. `waiting_on` covers personal-context blocking.

`domain/todo.rs` + `tools/todo.rs` — session-scoped working focus list (roadmap §2/§8; shaped after hermes `todo_tool` / Claude Code `TodoWrite`)
- `TodoItem { content, status: pending|in_progress|completed|cancelled, active_form }`; list order = priority; at most one `in_progress` (validated on write)
- distinct from `task`: a todo dies with the conversation. Persisted per session (`SessionTodoRecord`, keyed by session id) because shion reloads a session each turn, but it is disposable — the dispatcher clears it on `/new`
- `todo` tool: call with no args to read; pass `todos` to replace the whole list (full-list replace, no merge). Reads the current session from the ambient turn context (`current_session`); inert (no session) for aux sub-agents and sweeps
- the turn's session context is established for BOTH paths: the gateway dispatcher sets it (with a real `ReplySink`), and `AgentRuntime::handle_input` sets a *detached* context (no-op sink) when none exists, so the REPL gets `todo` too — see `SessionContext::detached`

`domain/memory.rs` + `tools/memory.rs` + `infra/memory/memory_db.rs` — long-term memory as three surfaces (roadmap §6/§1; design in `docs/memory-injection-plan.md`)
- `Memory` model is governed and scoped: `kind` (profile/preference/feedback/project/person/fact/decision/reference), `status` (candidate→active, plus archived/rejected), `confidence` (extracted/inferred/confirmed/user_written), `importance`, `pinned`, `scope` (`MemoryScope` global/project/channel/session, serialized as `scope_type`+`scope_key`), `source`/`source_message_id`, timestamps, `expires_at`/`last_used_at`. `MemoryContext::from_session` derives the turn's `allowed_scopes` from the session id (chat → global+channel+session; CLI → global+session, **never** infers project from chat)
- **L1 pinned** (done): `MemoryRepository::pinned(ctx)` filters `is_pinnable` (pinned + active + confirmed/user_written + identity-kind + in-scope); `system_prompt::render_pinned_memory_block` renders an ≤800-char block injected in `infra/llm.rs::complete` **after** the volatile tier (cache-stable), marked `<!-- shion:memory:pinned -->`, flagged as untrusted data. Main agent only (`build_llm(..., Some(repo))`); aux/delegate get `None`
- **L2 tool/governance** (done): `memory` tool `save/search/list/update/promote/reject/archive`; `search` is scope-bounded (`MemoryQuery` + `rerank_score`: lexical `LIKE` + importance/confidence/recency, no embedding). Operator CLI `shion memory list/search/promote/reject/pin`. `pin` is the manual-only path into L1 — automated extraction never pins
- reviewer writes extractions as `candidate + extracted`, scoped to the origin channel, deduped via `find_by_source_message_id` (same governance as task inbox — user triages candidates up to active/pinned)
- **L3 active recall** (done): `MemoryRepository::recall(ctx, text, limit)` scores active, in-scope memories against the turn's user message by **token overlap** (`recall_terms` = ASCII words + CJK bigrams + stopword filter; `recall_score`), distinct from L2 `search`'s whole-query substring match. Top `RECALL_LIMIT`=5 rendered by `system_prompt::render_recalled_memory_block` into an ≤2000-char block (each line `source:`-tagged, untrusted caveat, `<!-- shion:memory:recall -->`), injected in `infra/llm.rs::complete` **after** pinned (fixed `volatile | pinned | recall` order; pinned hits deduped out of recall). Recall failure is non-fatal but `warn!`-logged. Surfaced memories get `last_used_at` stamped via `MemoryRepository::mark_used` (only touches `last_used_at`, not `updated_at`) on a spawned best-effort task off the reply path — a Phase 4 usage signal

`domain/run.rs` + `RunRepository` (impl in `infra/persistence/db.rs`) — the **run ledger**: an execution/audit record of every agent turn (roadmap §7)
- one `Run` per turn (`id`, `session_id`, `input`, `plan` summary, `status` running/done/failed, `final_output`, `error`, timestamps) and one `RunStep` per tool call (`seq`, `tool_name`, `args`, `result`, `error`, `ok`, timestamps). Lives in `shion.db` — execution state bound to a session, disposable like messages, **not** durable personal data
- steps are captured at `execute_isolated` (see `services/tool_registry.rs`), so the ledger covers LLM-driven and keyword-routed tool calls alike. `RunContext` carries a shared `seq` counter so steps order stably even across the tool's spawned task
- every write is best-effort (warn-logged, never fails a turn or a tool) — same contract as memory `mark_used`
- **redaction**: step `args` are stored verbatim *except* each `Tool` may scrub its own via `Tool::redact_args` (default identity) — `shell` strips secret-looking substrings (`key=value`, `Bearer`, `--password`, high-entropy tokens), `file` drops the write `content` body. `result` is truncated but not scrubbed (shell *output* can still contain secrets — accepted, `shion.db` is local/disposable). Fields are length-capped (`RUN_FIELD_CAP`/`STEP_FIELD_CAP`)
- aux sub-agents and maintenance sweeps run without a `RunContext`, so their tool use never enters the ledger
- operator view: `shion run list [--limit N]` / `shion run inspect <id>` (`cli/inspect.rs`)
- deliberately NOT in v1: a `recoverable` flag / `resume` (no consumer yet — roadmap §6's "no dead fields"); ledger pruning (runs accumulate like messages; a later operator action can trim)

`cli/chat.rs` — wires everything together; creates `Arc<Db>` and passes it as both repos
- Session ids are program-managed (uuid v7); every run starts a fresh session. `/new` and `/clear` are equivalent — both open a new session. There is no user-supplied session id and no `/session` subcommand.

`agent/daemon.rs` — background maintenance supervisor, hosted by the gateway (pattern borrowed from gbrain's `autopilot` supervisor)
- `Schedule` wraps `croner` (5-field Unix cron, e.g. `0 * * * *`); `Maintenance` trait is the scheduled unit of work
- `ReviewSweep` is the one fixed action: run the reflective reviewer over every stored session with ≥1 user turn. Beyond memories/skills, the reviewer also extracts commitments ("I'll do X", "waiting on Y") and captures them as `inbox` tasks tagged with the origin `source` + a content-derived `source_message_id` dedup key (`find_by_source_message_id` guards against re-capturing across sweeps). Auto-extracted tasks only ever land in `inbox`, never `todo`, and extracted memories land as `candidate` (scoped to the origin channel, deduped by `find_by_source_message_id`), never pinned/active — the user triages both up the ladder (`shion task` / `shion memory promote|pin`).
- `ReminderSweep` delivers due reminders via `Notifier` every minute (10-min grace window; older ones are marked `missed`)
- `TaskSweep` notifies once when an open task comes due (the task stays open; `due_notified_at` is the at-most-once guard)
- `BriefingSweep` is the opt-in daily briefing (roadmap §4): it reads open tasks + recently-learned memories, lets the aux LLM compose a short digest (`briefing_prompt` is the pure, clock-injected prompt builder — returns `None` when there's nothing worth a ping), and delivers it through the same `Notifier`. Only scheduled when `briefing_schedule` is set (no default — proactive pings stay opt-in); wired in `cli/gateway.rs`.
- `supervise` is the loop: sleep to the next cron fire, run the cycle, isolate per-cycle failures, and trip a circuit breaker after 5 consecutive failures
- the OS-level supervisor install is `cli/service.rs` (`shion gateway start/stop/restart/status`, macOS launchd: `KeepAlive` auto-restart + `RunAtLoad`)

`agent/gateway.rs` — always-on gateway (pattern borrowed from hermes-agent's gateway: a persistent process hosting background services + ingress)
- `MessageHandler` (`domain/gateway.rs`) is the pure seam between a transport and the agent; `AgentRuntime` implements it (an inbound message is one session turn)
- `Channel` trait = a pluggable ingress; `Gateway` hosts N channels + N `MaintenanceService`s (the `daemon.rs` supervisor loop — review sweep on the config schedule, reminder + task sweeps every minute, optional daily briefing), all sharing one `watch` shutdown signal
- channels are declared in `~/.shion/config.toml` and constructed in `cli/gateway.rs`; `feishu`, `telegram`, `wechat`, and `homeassistant` (event ingress) are the wired channels
- sender admission is two-layered: each channel's `admit` filters message shape (non-text, bot senders, group mention gate), then the shared `PairingGuard` (`agent/pairing.rs`, store in `domain/pairing.rs`) decides identity — config `allow_from` is pre-trusted, approved pairings pass, anyone else gets a pairing code (`shion pair approve <code>` on the host admits them; `cli/pair.rs`)
- `GatewayDispatcher` (`agent/interaction.rs`) is the front door between a channel and the agent: a channel builds a `ReplySink` (`domain/gateway.rs`) for the chat and hands it each inbound message; the dispatcher classifies chat control commands and otherwise runs a turn. Channels no longer await turns or send agent replies themselves — the dispatcher owns that, and runs each turn on a spawned task so the receive loop keeps polling (which is what lets an `/approve` reply arrive mid-turn). One turn at a time per session.
- chat control commands (any channel): `/new` (also `/clear`, `/reset`) rotates the session hermes-style (`SessionRepository::rotate` archives the old transcript under a fresh id, leaving the chat's session empty — the reviewer can still see it), clears approval state, and clears the session's working todo list; `/approve` (+ `/approve session`) and `/deny` resolve a pending approval; `/sethome` (also `/home`) makes the current chat the home channel for proactive output (persisted via `HomeRepository`, `domain/home.rs`); `/wechat login` (also `/weixin`) provisions the WeChat channel by sending its login QR **into the current chat** as a photo — so an already-working channel (e.g. Telegram) sets up WeChat with no host shell. It drives the `WeChatLogin` trait (`domain/gateway.rs`, impl `WeChatQrLogin` in `infra/messaging/wechat.rs`), which writes creds and pulses a `Notify` the WeChat channel's `serve` loop is waiting on, so it comes online without a restart
- home channel + shutdown notice (hermes-borrowed): a single `HomeNotifier` (`infra/messaging/home_notifier.rs`) delivers all proactive output — reminders, task due notices, and the gateway's shutdown notice. It resolves the home at notify-time: the `/sethome` override (db, a `{platform}:{chat_id}` session id) wins over the config `home_chat` fallback (feishu first), degrading to the macOS notifier when no chat home resolves. On shutdown the gateway sends an "offline" notice through it (bounded by `SHUTDOWN_NOTICE_TIMEOUT`) before tearing down — only wired when a chat channel exists, so a foreground Ctrl-C with no channels stays quiet
- interactive tool approval over chat (ported from hermes' gateway approval): the gateway wires `ChatApprover` (`agent/interaction.rs`), not a deny-everything approver. When a side-effecting tool requests approval (`Risk::Normal`/`Dangerous`), the agent sends a prompt to the chat and the turn suspends on a `oneshot` registered in the shared `ApprovalState` (keyed by session, 5-min timeout); the user's `/approve`/`/deny` resolves it. `Risk::Safe` actions run without asking. With no chat session in context (maintenance sweeps, aux sub-agents) approval is denied. The turn's session context (id + `ReplySink`) reaches the approver via a task-local in `services::tool_registry` that `execute_isolated` re-establishes across its `tokio::spawn`.
- background install: `shion gateway start` (see `cli/service.rs`) runs it under launchd; bare `shion gateway` is the foreground process launchd invokes

`infra/messaging/feishu.rs` — the feishu integration: `FeishuChannel` (ingress), `FeishuSender` (outbound: cached tenant token + send; also a `TextSender` for the shared `HomeNotifier`)
- receives `im.message.receive_v1` over Feishu's WebSocket long connection (open-lark, no public callback URL needed); replies via the IM REST API with plain reqwest
- the ws connection runs on a dedicated thread with a current-thread runtime because open-lark's event dispatcher is not `Send`; events cross back over an mpsc channel
- `admit` filters message shape: `require_mention` for group chats, non-text and bot-sent messages dropped; sender identity goes through the shared `PairingGuard`
- session id is `feishu:{chat_id}`, so each chat is one continuous session; group @mention placeholders are stripped

`infra/messaging/telegram.rs` — the telegram integration: `TelegramChannel` (ingress), `TelegramSender` (outbound send; also a `TextSender` for the shared `HomeNotifier`)
- receives messages via `getUpdates` long polling (no public callback URL needed); plain reqwest against the Bot API, no SDK dependency
- `admit` mirrors the feishu policy: `require_mention` (group text must contain `@bot_username`, resolved via `getMe` at startup), non-text and bot-sent messages dropped; sender identity goes through the shared `PairingGuard`
- session id is `telegram:{chat_id}`; replies are sent with `parse_mode=Markdown` (rich formatting), falling back to plain chunked text when the API rejects the Markdown or the reply exceeds 4096 UTF-16 units

`infra/messaging/wechat.rs` — the WeChat (微信) integration over the **iLink** personal-bot protocol, built on the `wechatbot` crate (HTTP/JSON long-polling against `ilinkai.weixin.qq.com`, no public callback URL). `WeChatChannel` (ingress) + `WeChatSender` (outbound, also a `TextSender`) **share one `WeChatBot` instance** (built by `build_bot`, wired in `cli/gateway.rs`) — required because the crate keeps each user's reply `context_token` in memory, populated by the poll loop, and `send` needs it.
- the crate owns its own poll loop (`WeChatBot::run`) and fires a **synchronous** `on_message` callback, so the channel adapts rather than drives: the handler clones the message and `tokio::spawn`s the async pairing + `dispatcher.handle`, then `serve` hands the thread to `run()` under a shutdown `select!` (dropping the `run()` future cancels the poll)
- login is **QR-based**; creds → `~/.shion/wechat/credentials.json`. Provision either on the host with `shion wechat login` (`cli/wechat.rs`, renders the QR in-terminal via the `qrcode` crate) or from chat with `/wechat login` (the QR is sent into the chat as a photo — see the chat-commands list). `WeChatChannel::serve` **waits** for the cred file on an `Arc<Notify>` shared with `WeChatQrLogin` (it doesn't die without creds), so a chat-provisioned login brings the channel online with no restart. QR→PNG is `render_qr_png` (qrcode matrix → `image` crate, png feature only); photo delivery is `ReplySink::send_photo` (default errors; Telegram overrides it via `sendPhoto`)
- **DM-only**: an iLink bot identity can't join ordinary WeChat groups, so there's no group/mention gate — `PairingGuard` (`platform = "wechat"`) is the only admission control. Session id is `wechat:{user_id}`
- known limitation: proactive output (reminders/briefing via `HomeNotifier`) reaches a user only after they've messaged the bot since process start (the `context_token` map is in-memory, not persisted). The `wechatbot` crate also forces `reqwest`'s default TLS (native-tls/openssl) rather than shion's rustls — accepted tech-debt; switching needs a vendored patch

`cli/gateway.rs` — wires the `gateway` subcommand; `cli/wiring.rs` — shared `AgentRuntime` construction used by both chat and gateway (differ only in the `Approver`)

## Key extension points

- **Add a tool**: implement `Tool` in `src/tools/`, register it in `cli/chat.rs`
- **Swap LLM provider**: implement `LlmClient` (`domain/llm.rs`) for another backend and construct it in `cli/chat.rs`
- **Swap persistence**: implement `SessionRepository + MessageRepository` for a different backend; no changes needed in `agent/` or `domain/`
- **Add agent-loop control** (clarify / retry / budget — roadmap §8): build it as a layer in `AgentRuntime::turn_body` *above* `LlmClient`, or drive the tool-call loop in-house instead of delegating it to rig's `agent.prompt().max_turns()`. There is no planner to subclass — the loop currently lives inside rig
- **Change the scheduled action**: implement `Maintenance` (`agent/daemon.rs`) and construct it in `cli/gateway.rs`
- **Add a gateway ingress**: implement `Channel` (`agent/gateway.rs`) for a new transport (TCP/HTTP/chat platform), `add_channel` it in `cli/gateway.rs`, gated by a `~/.shion/config.toml` declaration — `infra/messaging/feishu.rs` is the reference implementation

## Testing

Tests live beside the code with `#[cfg(test)] mod tests`. Use `#[tokio::test]` for async. Name tests by behavior (`time_tool_returns_non_empty_string`).

## Coding style

Default Rust formatting (`cargo fmt`), `snake_case` for modules/files/functions, `PascalCase` for structs and enums. CLI subcommands stay short and verb-based. Prefer small modules with one responsibility; keep async database code close to the layer that owns it.

## Commit & PR style

Short imperative commit messages: `add file tool`, `wire llm client`. PRs include a concise description, commands run for verification, and terminal output when CLI behavior changes.
