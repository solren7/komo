# AGENTS.md

Guidance for coding agents working in this repository.
`CLAUDE.md` is a symlink to this file — edit `AGENTS.md` only.

komo is a personal-agent framework in Rust (DDD-style layers) plus a bun
workspace of JS/TS clients under `apps/`. Building needs `protoc`
(`brew install protobuf` — feishu websocket frames are protobuf).

## Commands

```bash
cargo check / build / fmt
cargo test --workspace             # REQUIRED: bare `cargo test` skips komo-core's ~70 tests
cargo test tools::time             # single module

komo init                          # scaffold ~/.komo (config.toml/.env/SOUL.md/USER.md; never overwrites)
cargo run -- chat                  # full-screen TUI (needs a terminal; scripts use the api channel)
cargo run -- gateway               # always-on process: sweeps + channels (feishu/telegram/wechat/HA)
komo gateway start|stop|restart|status   # macOS launchd supervision
komo upgrade [--no-restart]        # git pull --ff-only + cargo install + restart gateway
komo logs [-n N] [-f] [--stdout]   # tail gateway tracing log
komo doctor                        # config & gateway health
komo health                        # liveness probe (exit 0 = healthy; Docker HEALTHCHECK)

komo memory list|search|promote|reject|pin|triage|report
komo dream [--apply]               # usage-driven candidate consolidation (preview by default)
komo cron list|add|add-agent|run|enable|disable|remove
komo run list|inspect|resume|prune # run ledger (⟲ = recoverable)
komo skills list|install|inspect|promote|reject|protect|unprotect|enable|disable|audit
komo policy list|check|saved       # permission policy + saved grants
komo journey                       # learning timeline (memories + skills)
komo channel list|probe|setup      # channel inventory / verification / interactive setup
komo channel wechat login          # provision WeChat creds via QR (on the host)
komo pair approve|revoke|list      # admit chat senders
komo task list                     # kanban tasks
komo workday [YYYY-MM-DD]          # Chinese working-day check (holidays + 调休)
```

Logs: `init_tracing` in `main.rs` installs the subscriber (without it every
`info!` is a no-op). Gateway tees stderr into daily-rotated
`~/.komo/logs/gateway.YYYY-MM-DD.log` (what `komo logs` reads). Level via
`KOMO_LOG` (default `info,toasty=warn,rig_core=warn`; set `KOMO_LOG=debug` to
see full tool results). Turns run in `run` spans, tool calls in `tool` spans,
matching the run ledger.

## Data & storage rules

| File | Contents | Durability |
|---|---|---|
| `~/.komo/state.db` | sessions, messages, todos, reminders, pairings, settings, run ledger | disposable — delete freely |
| `~/.komo/kanban.db` | cross-session tasks | durable |
| `~/.komo/memory.db` | long-term memories | durable |
| `~/.komo/cron.db` | scheduled cron jobs | durable |
| `~/.komo/permissions.json` | saved approval grants | durable |
| `~/.komo/tool-output/` | over-limit tool results (7-day retention) | disposable |
| `~/.komo/skills/` | skill files (filesystem is the source of truth) | durable |

Schema-change rules (toasty's `push_schema` runs only for **new** db files, and
is not idempotent):

- New table / non-additive change on disposable state → delete the affected
  file (`TaskRecord`→kanban.db, `CronJobRecord`→cron.db, anything else incl.
  `RunRecord`/`RunStepRecord`→state.db).
- **Column additions never need a reset**: `infra/persistence/mod.rs::ensure_columns`
  ALTERs in place on connect. Extend `EXPECTED` in `memory_db.rs` for
  `MemoryRecord` columns, and the matching list in `db.rs::connect` for
  state.db (`SESSION_COLUMNS` / `MESSAGE_COLUMNS` / `RUN_COLUMNS` /
  `STEP_COLUMNS`). Columns must be NOT NULL + DEFAULT, or nullable.
  Durable data (memory.db) must **only** ever change additively.

Turso/toasty invariants (`infra/persistence/`, `infra/memory/memory_db.rs` —
the only places the ORM appears; model structs private to their file):

- Backend is Turso in MVCC `concurrent_writes` mode; no `rusqlite`. DB URL is
  `turso:<path>` / `turso::memory:`.
- MVCC rejects `AUTOINCREMENT` → every key is a `String` UUIDv7, never `#[auto]`.
- Conflicting commits fail and must be retried: wrap single-write mutations in
  `with_write_retry`; multi-write sequences in a real transaction *inside*
  `with_write_retry` (rollback + clean re-run, never double-apply).
- Legacy rusqlite files auto-migrate once (staged to `.sqlite-backup`, `.turso`
  marker prevents re-migration).

## Gateway ↔ CLI coexistence

Turso holds an exclusive cross-process lock per db file. While the gateway
runs, the CLI cannot open the dbs directly — every operator action goes through
`services/operator_control/`: probe `~/.komo/gateway.json` (rendezvous file) →
route over the loopback api channel (`infra/messaging/api.rs`,
`infra/gateway_client.rs`) or fall back to direct db open. **Both paths run the
same `operator_control/actions.rs::OperatorActions`**, so business logic can't
fork — add new operator actions there, not in the CLI or api handlers.

- `komo chat` → `POST /v1/chat/completions` with `X-Komo-Trusted` (loopback
  only): side-effecting tools auto-approve for the host operator. API sessions
  are stored as `api:<uuid>` internally, while `X-Komo-Session-Id` carries the
  bare UUID; the gateway accepts the old prefixed form for compatibility.
- Cancel: `POST /api/interactions/{session}/cancel` flips the session's
  `CancelSignal`; `run_agent_loop` races every await against it. A running tool
  stops only if it claims `ToolContext::cancelled()` (shell kills its process
  group; web_fetch/web_search drop the request; fs tools deliberately run to
  completion so `apply_patch` never half-applies). Cancelled runs are Failed,
  **not** recoverable.
- api channel is loopback/ephemeral by default; `[channels.api] enabled = true`
  + `API_SERVER_KEY` widens it. `web_dir` serves the built SPA same-origin;
  `remote_interactive = true` lets keyed remote callers run interactive turns
  (`X-Komo-Trusted` stays loopback-only regardless). CORS grants loopback
  origins + Electron's `null` origin; bearer key remains the gate.

## Config

`~/.komo/config.toml` = runtime settings (provider/model/`models`/aux_model,
`schedule`, `briefing_schedule` + `briefing_workdays_only`, `dream_schedule`
(default nightly `0 3 * * *`, `"off"` disables), `[channels.*]`, `[policy]`).
`~/.komo/.env` = credentials only. Precedence: defaults < config.toml <
`KOMO_*` env. `KOMO_HOME` relocates the directory.

Resolution happens **once** in `src/config/` into a `ConfigSnapshot`; problems
become `ConfigIssue`s (never abort resolution) checked by `validate_agent` /
`validate_gateway`. Two deliberate warnings, not fatals: missing model API key
(boots with `UnconfiguredLlm` that errors per call) and HA channel without
`HASS_TOKEN` (channel offline, others unaffected). **Never re-read config.toml
or call `std::env::var` in callers** — the only exception is `KOMO_HOME`.

Channels (`[channels.feishu|telegram|wechat|homeassistant]`): behavior keys in
the table, credentials in `.env`. `allow_from` pre-trusts senders; everyone
else must pair (`komo pair approve <code>`; codes stored salted-hashed,
rate-limited, expire in 1h). WeChat is QR-login (creds in
`~/.komo/wechat/credentials.json`), DM-only, and can't deliver proactive output
until the user messages the bot after process start. `home_chat` is the
fallback for proactive output; a `/sethome` chat command override (db) wins.

Model menu: `models = [...]` declares what a session may switch to; entries may
be provider-qualified (`deepseek:deepseek-chat`) and `ModelConfig::menu()`
drops entries whose provider has no key (except the running `model`). Choice is
carried per turn in `X-Komo-Model`/`X-Komo-Effort`, validated against the menu,
stored on the session; `RoutingLlm` dispatches across providers. Effort levels
are per-provider (`Provider::efforts` ↔ `reasoning_params` must agree — there
is a test). **Invariant: every aux path (reviewer, delegate, recall, sweeps)
builds a synthetic `Session` with empty overrides** — that's what keeps a
conversation's model from leaking onto the aux model; preserve it when adding
aux callers.

The `codex` provider authenticates from the Codex CLI's OAuth file
(`~/.codex/auth.json`, auto-refreshed) instead of an env key, and requires
streaming — see `infra/codex.rs`.

## Architecture

```
CLI/channel → AgentRuntime ─ run_agent_loop ─┬→ LlmClient::begin_turn → TurnDriver (ONE rig completion / round)
                                             └→ ToolExecutor::execute_round → tools   (loop until Step::Final)
                          ↘ MessageRepository · RunRepository (ledger) → Response
```

komo owns the tool loop: rig does a single completion per round;
`run_agent_loop` (`agent/runtime.rs`) is where round-level control lives
(`max_turns` budget, cancellation, clarify). Tool errors return as outcome
content the model can recover from; only a driver/LLM error aborts the turn.

**Module map** (one line each; read the module for details):

- `domain/` — pure traits + value types, no I/O, no external crates
  (`Tool`, `LlmClient`/`TurnDriver`, repositories, policy engine, pairing).
- `agent/runtime.rs` — session lifecycle + the tool loop; loads only a recent
  transcript window per turn (`find_windowed`); wraps each turn in a ledger
  `Run` (all ledger writes best-effort, never fail the turn).
- `infra/llm.rs` — `RigLlm<M>` over rig; `assemble` builds the tiered system
  prompt once per turn (stable tier incl. `~/.komo/USER.md`, then memory
  prefix from `MemoryEnricher` — main agent only). `RoutingLlm` = cross-provider
  dispatch. Codex is the streaming exception.
- `services/tool_execution/` — `ToolExecutor::execute_round`: per call, claim
  ledger seq → redact args → run with panic catch + `tool` span →
  transient-retry (connection errors retry anything; ambiguous only
  `Tool::idempotent()`) → bound the LLM-facing result via
  `services/tool_output_store.rs` (full text on disk, head+tail preview) →
  record `RunStep`. Policy is instance-owned `ToolExecutionConfig`;
  `Tool::max_duration()` overrides the per-call timeout (approval-gated tools
  must outlast the 5-min approval prompt, `APPROVAL_BOUND`).
  `Tool::call(Value, &ToolContext)` is the **only** tool entry point; the
  `SESSION` task-local serves the approvers only — tools take `ctx.session`.
- `tools/` — `time`, `shell` (own process group, hardline floor no approval
  unlocks, nested timeouts), `grep`/`glob` (ripgrep libraries in-process;
  policy runs over paths **before** content is read), `read`/`write` +
  `fs_common` (workspace-confined; `write_if_unchanged` guards the approval
  window), `edit` (exact match only, no fuzzy) / `apply_patch` (v2 envelope,
  one approval per batch, no rollback — reports exactly what landed),
  `web_fetch` (content-type gated, 256 KB download cap, deny-only network
  policy), `homeassistant` (`call_service` approval-gated; `BLOCKED_DOMAINS`
  hardline), `task`, `todo` (session-scoped, dies on `/new`), `memory`,
  `skill`, `cron`, `delegate`, `ask_user` (clarify).
- `tools/delegate.rs` — sub-agent as a real agent turn on a `delegate:<uuid>`
  session; inherits the parent's ambient session context (approvals prompt the
  real conversation, cancel propagates); recursion blocked structurally
  (sub-agent tool set has `delegate: None`); each delegation is its own ledger
  run. The unattended cron runtime gets no `delegate`.
- `domain/policy.rs` + `agent/policy_approver.rs` — permission policy. Ladder,
  strongest first: **tool hardline floor > config deny > saved grant > config
  allow / `default_normal` > ask**. Saved grants (`permissions.json`, written
  only by `PolicyApprover`) never cover `Risk::Dangerous` and are never read
  unattended. Unattended contexts (cron/briefing/sweeps) grant only through
  `unattended = true` allow rules. Read-only actions (`read`, `web_fetch`) are
  deny-only — never prompted. Wholly-denied tools are dropped from the catalog
  at wiring (`drop_policy_denied`). Policy only tightens; hardline floors
  short-circuit inside the tool.
- `domain/memory.rs` + `services/memory_enrichment.rs` — three surfaces:
  L1 pinned block (manual `pin` only), L2 `memory` tool + operator CLI,
  L3 recall (lexical token overlap; fetch 15, inject ≤5, aux-screened above 5;
  injected hits get `recall_count`/`last_used_at`/query-hash stamped —
  dreaming's signals). Nightly `DreamSweep` promotes candidates recalled ≥3
  times by ≥2 distinct queries, archives 30-day-cold ones; only candidates are
  touched. Reviewer extractions are always `candidate`, never pinned/active.
- `domain/run.rs` — run ledger: one `Run` per turn, one `RunStep` per call.
  `elapsed_ms` is the duration field (`started_at`/`ended_at` are whole
  seconds); 0 / empty `structured` read as *unknown/absent*, never
  instant/empty-object. Args redacted per-tool (`Tool::redact_args`); results
  truncated not scrubbed. `komo run resume` re-dispatches a *fresh* primed
  turn (the ledger is an audit record, not a checkpoint); `recoverable` is set
  only by crash reconciliation, cleared at-most-once, never auto-resumed.
- `domain/skill.rs` + `infra/skills.rs` + `services/skill_registry.rs` —
  skills are `SKILL.md` files under `~/.komo/skills/` (active) and
  `.candidates/` (proposals). Automated writes (`save` — reviewer + `skill
  learn`) only ever produce candidates; `install` is the human-in-the-loop
  exception that lands active. `protected` skills refuse even proposals.
  `SkillRegistry` re-scans dirs on every query (no restart needed); only the
  capped prompt catalog is a startup snapshot (cache stability).
- `agent/daemon.rs` — `Maintenance` sweeps under `supervise` (circuit breaker
  after 5 failures): `ReviewSweep` (via the shared `ReviewCoordinator`, which
  also serves the post-turn trigger — watermark + in-flight guard prevent
  duplicate reviews), `ReminderSweep`, `CronJobSweep` (claim-before-run: a
  crash never re-fires a slot), `TaskSweep`, `BriefingSweep` (opt-in; aux-model
  runtime with read-only tools + deny-all unattended approver; degrades to
  tool-less `complete` on error), `DreamSweep`. `WorkdayGated` decorator gates
  a sweep to Chinese working days (`infra/workday.rs`, cached per-year).
- `agent/gateway.rs` + `agent/interaction.rs` — gateway hosts channels +
  sweeps. `GatewayDispatcher` owns turns (spawned per turn so `/approve` can
  arrive mid-turn; one turn per session). Chat commands: `/new` (rotate
  session, clear todos + approval state), `/approve [session|always]`,
  `/deny`, `/sethome`, `/wechat login`. `ChatApprover` suspends the turn on a
  oneshot (5-min timeout); no session in context ⇒ deny. `HomeNotifier`
  delivers all proactive output (sethome override > config `home_chat`,
  feishu first > macOS notification).
- `infra/messaging/` — channels: feishu (ws long connection on a dedicated
  thread), telegram (long polling, Markdown with plain-text fallback), wechat
  (iLink, DM-only, shared `WeChatBot` instance, in-memory reply tokens),
  homeassistant (event ingress, closed-by-default filters, no pairing,
  approvals denied). Session ids: `{platform}:{chat_id}`.
- `cli/wiring.rs` — shared `AgentRuntime` construction (chat vs gateway differ
  only in `Approver`); register new tools here.
- `tui/` — ratatui chat front end over gateway-or-in-process backends; state +
  key handling terminal-free in `tui/app.rs`. `komo resume <id>` (or the
  compatible `komo session resume <id>`) re-enters a session; a bare API UUID
  resolves its internal `api:<uuid>` id and hydrates the transcript.
- `cron` (`~/.komo/cron.db`, `CronJobSweep`) — two job modes: **command**
  (operator-authored, runs directly, no approver) and **agent** (unattended
  turn on `cron_runtime`, side effects need `unattended = true` policy rules).
  Chat-created jobs (`tools/cron.rs`) are approval-gated at creation; a
  command job from chat is `Risk::Dangerous`. Recurring *work* = cron job,
  recurring *message* = reminder.
- `apps/` — bun workspace: `apps/app` (shared React renderer) mounted by
  `apps/desktop` (Electron) and `apps/web` (SPA served via `web_dir`). Talks
  to the gateway over HTTP only (`HttpKomoClient`); feature-first layout;
  react-query for server state, zustand for client state; thread is
  assistant-ui over an async-generator adapter. Components may only use
  semantic theme tokens — `bun run lint` fails on raw colors. Commands:
  `cd apps && bun install`, `bun run check` (typecheck + lint + fmt + test).
  Conventions: `apps/app/README.md`.

## Extension points

- **Add a tool**: implement `Tool` in `src/tools/`, register in `cli/wiring.rs`
  (and add it to `tool_execution::policy_scope` if it should be policy-filterable).
- **Swap LLM provider**: implement `LlmClient` (`domain/llm.rs`), construct in
  `build_llm`.
- **Swap persistence**: implement the repository traits; `agent/`/`domain/`
  need no changes.
- **Agent-loop control**: add round-level control points in `run_agent_loop`;
  extend `TurnDriver`/`Step`, not rig. Clarify (`tools/ask_user.rs` +
  `services/clarify.rs`) is the sentinel-tool reference.
- **Scheduled action**: implement `Maintenance`, construct in `cli/gateway.rs`.
- **Gateway ingress**: implement `Channel`, `add_channel` in `cli/gateway.rs`,
  gate behind a `[channels.*]` declaration — feishu is the reference.

## Testing

Tests live beside the code (`#[cfg(test)] mod tests`, `#[tokio::test]` for
async), named by behavior. **Always `cargo test --workspace`** — the bare root
command skips `crates/komo-core`.

## Coding style

`cargo fmt` defaults; `snake_case` modules/functions, `PascalCase` types. Small
modules, one responsibility; keep async db code in the layer that owns it. CLI
subcommands short and verb-based.

## Commit & PR style

Short imperative commits (`add file tool`). PRs: concise description, commands
run for verification, terminal output when CLI behavior changes.

## Repo docs

- Issues/PRDs: local markdown under `.scratch/<feature-slug>/` — `docs/agents/issue-tracker.md`
- Triage labels: `needs-triage` / `needs-info` / `ready-for-agent` / `ready-for-human` / `wontfix` — `docs/agents/triage-labels.md`
- Domain docs: `CONTEXT.md` + `docs/adr/` — `docs/agents/domain.md`
- Long-form design rationale (archived old AGENTS.md): `docs/agents/architecture-notes.md`
