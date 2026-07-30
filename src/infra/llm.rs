use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use futures_util::StreamExt;
use rig::{
    OneOrMany,
    client::{ClientBuilder, CompletionClient},
    completion::{
        AssistantContent, CompletionModel, CompletionRequestBuilder, GetTokenUsage,
        Message as RigMessage, ToolDefinition, Usage,
        message::{ToolResultContent, UserContent},
    },
    providers::{anthropic, deepseek, openai, openrouter},
};
use serde_json::{Value, json};

use crate::{
    config::{ModelConfig, Provider, split_model_id},
    domain::{
        llm::{LlmClient, Step, TokenUsage, ToolCallReq, ToolOutcome, TurnDriver},
        message::{Message, Role},
        session::Session,
    },
    infra::codex::{CODEX_BASE_URL, CodexAuth, CodexHttpClient, codex_static_headers},
    services::{memory_enrichment::MemoryEnricher, tool_execution::retry::should_retry},
};

/// Produces the system prompt (preamble) on demand. Called once per user turn
/// so the prompt is rebuilt per session rather than baked once at startup —
/// the gateway is a long-lived process, so a baked prompt would freeze the
/// volatile tier (date) at boot. The factory's output is day-precision, so it
/// stays byte-identical across turns within a day (upstream prompt cache stays
/// warm) and self-heals across midnight.
pub type PreambleFn = Arc<dyn Fn() -> String + Send + Sync>;

/// Stand-in for a provider whose API key is missing (see [`build_llm`]):
/// construction always succeeds so a fresh install boots, and every call —
/// `begin_turn` inherits the default one-shot driver over `complete` — fails
/// with the fix. The error text reaches the user as the turn's reply.
struct UnconfiguredLlm {
    message: String,
}

#[async_trait]
impl LlmClient for UnconfiguredLlm {
    async fn complete(&self, _session: &Session) -> anyhow::Result<String> {
        anyhow::bail!("{}", self.message)
    }
}

/// Generic [`LlmClient`] over any `rig` completion model. The concrete provider
/// type is erased behind `Arc<dyn LlmClient>` by [`build_llm`].
///
/// komo talks to the provider's [`CompletionModel`] directly rather than through
/// rig's `Agent`: since 0.41 a configured `Agent` runs exclusively through rig's
/// own `AgentRunner` loop, and komo owns the tool loop (`run_agent_loop`), so the
/// raw per-request API is the matching seam. Everything an `Agent` used to carry
/// for us — model handle, preamble, tool schemas — lives here instead.
pub struct RigLlm<M: CompletionModel> {
    /// Handle for the configured model, minted once at startup — the one a
    /// session with no `model` override runs on.
    model: M,
    /// The provider client the `model` was minted from, kept so a turn whose
    /// session names a different model can mint a handle for it (`M::make`).
    /// Only the handle is swapped — tools and preamble stay the ones assembled
    /// at startup.
    client: Arc<M::Client>,
    /// Tool schemas advertised to the provider (name + description + parameters).
    /// Only the *declaration* goes over the wire: komo dispatches every requested
    /// call itself in `ToolExecutor::execute_round`, so rig never runs a tool.
    tools: Vec<ToolDefinition>,
    /// The configured model: what a session with no override runs on.
    default_model: String,
    /// Which provider this is, for mapping a session's reasoning-effort level
    /// onto request params (see [`reasoning_params`]).
    provider: Provider,
    /// Rebuilds the system prompt each turn (see [`PreambleFn`]).
    preamble: PreambleFn,
    /// Max prior messages replayed as history per turn (config
    /// `max_history_messages`; `0` = unlimited). The backstop against a
    /// long-lived chat session sending its entire transcript every turn — see
    /// [`RigLlm::assemble`].
    max_history_messages: usize,
    /// Byte budget for the replayed history (`0` = unlimited). The message-count
    /// window alone can't bound context: a handful of pasted logs or diffs blows
    /// past any token limit while sitting well inside the count. Applied after the
    /// count window, trimming from the oldest end.
    max_history_bytes: usize,
    /// Optional per-turn memory enrichment. `Some` only for the main agent —
    /// aux/delegate sub-agents must not be fed the user's memory library. The
    /// enricher owns the whole memory policy (selection, screening, rendering,
    /// usage tracking); this adapter only appends the finished prefix.
    enricher: Option<Arc<MemoryEnricher>>,
    /// Drive each round over the streaming API instead of one-shot `send()`.
    /// Required by the ChatGPT Codex backend (it rejects non-streamed requests);
    /// `false` for every other provider, which keeps the simpler non-streaming
    /// path. The streamed chunks are aggregated back into one assistant turn, so
    /// the rest of the loop is identical either way.
    stream: bool,
    /// Per-completion timeout. rig's default reqwest client sets no request
    /// timeout, so a hung provider request would await forever and wedge the
    /// turn in `running`; this caps each completion so a stall fails the turn
    /// cleanly instead. `None` = no timeout (config `llm_timeout_secs = 0`).
    timeout: Option<Duration>,
}

/// Total attempts for one model round-trip whose failure classifies as transient
/// (1 initial + retries). A constant rather than config, for the same reason the
/// tool executor's is: transient retry is an internal robustness backstop.
const LLM_RETRY_MAX_ATTEMPTS: usize = 3;
/// Backoff before each retry, indexed by retry number (last entry reused). Longer
/// than the tool executor's: the usual transient completion failure is provider
/// rate-limiting, which does not clear in a quarter second.
const LLM_RETRY_BACKOFF_MS: [u64; 2] = [500, 2_000];

/// Re-run `attempt` while its failure looks transient, bounded by
/// [`LLM_RETRY_MAX_ATTEMPTS`].
///
/// Without this, the single most failure-prone call in a turn was the only one
/// with no retry: tool calls have classified retry (`tool_execution::retry`),
/// while one 429 or connection reset on round 11 of a long tool chain threw away
/// all ten rounds of work and handed the user a failure placeholder. A completion
/// has no side effect that could double-apply, so it is idempotent by
/// construction — both connection-level and ambiguous failures are safe to
/// re-send, and the classifier is shared with the tool path so the two can't
/// drift on what "transient" means.
///
/// Deliberately nested *inside* [`with_timeout`] by every caller: the configured
/// timeout is then a budget for the whole round (attempts included), so retrying
/// can't multiply a turn's worst-case latency, and our own timeout error — whose
/// text would otherwise classify as ambiguous — is never itself retried.
async fn with_retry<F, Fut, T>(mut attempt: F) -> anyhow::Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<T>>,
{
    let mut retries = 0usize;
    loop {
        let error = match attempt().await {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        if retries + 1 >= LLM_RETRY_MAX_ATTEMPTS || !should_retry(&error, true) {
            return Err(error);
        }
        let delay = LLM_RETRY_BACKOFF_MS[retries.min(LLM_RETRY_BACKOFF_MS.len() - 1)];
        tracing::warn!(
            attempt = retries + 1,
            delay_ms = delay,
            error = %format!("{error:#}"),
            "transient LLM failure; retrying the completion"
        );
        tokio::time::sleep(Duration::from_millis(delay)).await;
        retries += 1;
    }
}

/// Run `fut` under `timeout` (if set), turning a stall into a clean error rather
/// than an indefinite await. Shared by the tool-less `complete` path and every
/// tool-loop round. Wraps [`with_retry`], so the budget covers every attempt of
/// one round rather than each attempt separately.
async fn with_timeout<F, T>(timeout: Option<Duration>, fut: F) -> anyhow::Result<T>
where
    F: Future<Output = anyhow::Result<T>>,
{
    match timeout {
        Some(d) => match tokio::time::timeout(d, fut).await {
            Ok(result) => result,
            Err(_) => anyhow::bail!(
                "LLM completion timed out after {}s (provider unresponsive; \
                 failing the turn instead of leaving it running — raise \
                 `llm_timeout_secs` / `KOMO_LLM_TIMEOUT_SECS` if this is too tight)",
                d.as_secs()
            ),
        },
        None => fut.await,
    }
}

/// Cross-provider dispatcher: one type-erased backend per provider, selected by
/// the session's model id.
///
/// This layer exists because [`RigLlm`] is generic over a *single* provider's
/// model type (`deepseek::CompletionModel` and `openai::CompletionModel` are
/// unrelated types), so within-provider switching can happen inside one `RigLlm`
/// but crossing providers cannot. A qualified id (`deepseek:deepseek-chat`) picks
/// the backend here; the bare remainder picks the model inside it.
///
/// An unqualified id — or one naming a provider this gateway has no client for —
/// falls through to the default backend rather than failing the turn: the api
/// channel already validates a client's choice against the advertised menu, so
/// reaching here with something unroutable means config changed under a stored
/// session, and running on the default is the recoverable answer.
struct RoutingLlm {
    by_provider: Vec<(Provider, Arc<dyn LlmClient>)>,
    default_provider: Provider,
}

impl RoutingLlm {
    fn route(&self, session: &Session) -> &Arc<dyn LlmClient> {
        let wanted = session
            .model_override()
            .and_then(|id| split_model_id(id).0)
            .unwrap_or(self.default_provider);
        self.backend(wanted)
            .or_else(|| self.backend(self.default_provider))
            .expect("routing llm always holds its default provider's backend")
    }

    fn backend(&self, provider: Provider) -> Option<&Arc<dyn LlmClient>> {
        self.by_provider
            .iter()
            .find(|(p, _)| *p == provider)
            .map(|(_, backend)| backend)
    }
}

#[async_trait]
impl LlmClient for RoutingLlm {
    async fn complete(&self, session: &Session) -> anyhow::Result<String> {
        self.route(session).complete(session).await
    }

    async fn begin_turn(&self, session: &Session) -> anyhow::Result<Box<dyn TurnDriver>> {
        self.route(session).begin_turn(session).await
    }
}

/// Extra answer budget granted on top of an Anthropic thinking budget, so the
/// model has room to write a reply after it finishes reasoning.
const THINKING_ANSWER_HEADROOM: u64 = 8_192;

/// Map a reasoning-effort level onto the provider's request params, or `None`
/// when this provider/level pair has no effect.
///
/// Which levels a provider offers is [`Provider::efforts`]; this is the other
/// half — how a level is actually spelled on the wire. Both paths merge the
/// result into the agent's `additional_params`, which every provider flattens
/// into the request body.
fn reasoning_params(provider: Provider, effort: &str) -> Option<Value> {
    let level = match effort.trim() {
        level @ ("low" | "medium" | "high") => level,
        _ => return None,
    };
    match provider {
        // The OpenAI Responses API (which Codex speaks too) and OpenRouter both
        // take `reasoning.effort` verbatim.
        Provider::OpenAi | Provider::OpenRouter | Provider::Codex => {
            Some(json!({ "reasoning": { "effort": level } }))
        }
        // Anthropic has no effort scale — it budgets thinking in tokens, so the
        // levels map onto budgets. The caller must also raise `max_tokens` above
        // the budget (thinking is charged against it): see `agent_for`.
        Provider::Anthropic => {
            let budget = match level {
                "low" => 4_096,
                "medium" => 10_240,
                _ => 24_576,
            };
            Some(json!({ "thinking": { "type": "enabled", "budget_tokens": budget } }))
        }
        // Only a thinking on/off flag; see `Provider::efforts`.
        Provider::DeepSeek => None,
    }
}

/// Shallow-merge `extra`'s top-level keys into `base` (extra wins). Anything
/// non-object on either side is replaced outright, which is all the agent's
/// `additional_params` ever holds.
fn merge_params(base: Option<Value>, extra: Value) -> Value {
    match (base, extra) {
        (Some(Value::Object(mut base)), Value::Object(extra)) => {
            base.extend(extra);
            Value::Object(base)
        }
        (_, extra) => extra,
    }
}

impl<M> RigLlm<M>
where
    M: CompletionModel + 'static,
    // The retained provider client crosses the gateway's per-turn tasks, so it
    // has to be shareable — every rig provider client is.
    M::Client: Send + Sync + 'static,
{
    /// Assemble this turn's `(preamble, prompt, history)`: split the session
    /// into the latest user prompt + prior history, rebuild the system prompt,
    /// and append the memory-enrichment prefix (main agent only). Run once
    /// per turn — never per tool-loop round (recall is keyed on the user
    /// message, and re-running it each round would churn the cached prefix).
    async fn assemble(
        &self,
        session: &Session,
    ) -> anyhow::Result<(String, String, Vec<RigMessage>)> {
        // The current prompt is the most recent user message; everything before
        // it forms the conversation history sent to the model.
        let last_user_idx = session
            .messages
            .iter()
            .rposition(|m| m.role == Role::User)
            .context("no user message to respond to")?;
        let prompt = session.messages[last_user_idx].content.clone();

        // Window the replayed history to the most recent `max_history_messages`
        // (0 = keep everything). Without this a long-lived chat session
        // (telegram/feishu/wechat are keyed by chat id and only rotate on an
        // explicit `/new`) would resend its entire transcript every turn —
        // unbounded token cost and latency, eventually overflowing the context
        // window. The stable system-prompt + memory prefix is untouched, so the
        // upstream prompt cache is unaffected by trimming the tail.
        let window = window_history(
            &session.messages[..last_user_idx],
            self.max_history_messages,
            self.max_history_bytes,
        );
        // Tool notes only ride along for the most recent turns (see
        // [`TOOL_NOTE_TURNS`]): find where that tail starts.
        let notes_from = tool_note_cutoff(window);
        let history: Vec<RigMessage> = window
            .iter()
            .enumerate()
            .filter_map(|(idx, m)| to_rig_message(m, idx >= notes_from))
            .collect();

        // Rebuild the system prompt for this turn. It rides on the per-turn
        // request rather than on shared state, so concurrent sessions in the
        // gateway stay independent.
        let mut preamble = (self.preamble)();

        // Memory injection (main agent only): the enricher returns the finished
        // pinned+recall prefix, appended after the volatile tier so the
        // stable+context+volatile bytes stay cache-stable. Enrichment failure
        // is absorbed inside the enricher (memory is background context — it
        // must never fail a reply).
        if let Some(enricher) = &self.enricher
            && let Some(prefix) = enricher.enrich(&session.id, &prompt).await
        {
            preamble.push_str("\n\n");
            preamble.push_str(prefix.as_str());
        }

        Ok((preamble, prompt, history))
    }

    /// Resolve this turn's model settings: the assembled preamble, then the
    /// session's own model / reasoning-effort choices.
    ///
    /// A model handle is cheap to clone (`Arc`-backed provider client + a model
    /// id), so per-session settings land on a private [`TurnModel`] — concurrent
    /// sessions in the gateway never see each other's.
    ///
    /// Only the *main* agent is ever handed a stored session: every aux path
    /// (reviewer, delegate, recall screening, sweeps) builds a synthetic
    /// `Session`, whose overrides are empty. That is what keeps a conversation's
    /// model choice from leaking onto the aux model.
    fn model_for(&self, preamble: String, session: &Session) -> TurnModel<M> {
        let mut turn = TurnModel {
            model: self.model.clone(),
            preamble,
            max_tokens: None,
            additional_params: None,
        };

        // A session's model may be provider-qualified (`deepseek:deepseek-chat`).
        // Routing on the prefix is `RoutingLlm`'s job — by the time we get here
        // the provider is already decided, so only the bare id matters. Stripping
        // it here (rather than rewriting the session upstream) avoids cloning the
        // whole transcript just to change one field.
        if let Some(name) = session
            .model_override()
            .map(|id| split_model_id(id).1)
            .filter(|name| *name != self.default_model)
        {
            turn.model = M::make(&self.client, name);
        }

        if let Some(params) = session
            .effort_override()
            .and_then(|effort| reasoning_params(self.provider, effort))
        {
            // Anthropic charges thinking against `max_tokens`, so a budget above
            // the cap is rejected outright — raise the cap to clear it.
            if let Some(budget) = params
                .get("thinking")
                .and_then(|thinking| thinking.get("budget_tokens"))
                .and_then(Value::as_u64)
            {
                let needed = budget + THINKING_ANSWER_HEADROOM;
                turn.max_tokens = Some(turn.max_tokens.unwrap_or(0).max(needed));
            }
            turn.additional_params = Some(merge_params(turn.additional_params.take(), params));
        }
        turn
    }
}

/// One turn's model settings: which handle runs it, the system prompt assembled
/// for it, and the request knobs the session's reasoning-effort choice implies.
///
/// This is the per-turn state rig's `Agent` used to hold for us. Requests are
/// built off it round by round, so a round is exactly one provider completion and
/// komo's loop stays in charge of what happens between rounds.
struct TurnModel<M: CompletionModel> {
    model: M,
    preamble: String,
    max_tokens: Option<u64>,
    additional_params: Option<Value>,
}

impl<M: CompletionModel> TurnModel<M> {
    /// Build one round's request: `history + prompt` under this turn's preamble,
    /// with `tools` advertised as declarations only (komo dispatches the calls).
    fn request(
        &self,
        prompt: RigMessage,
        history: Vec<RigMessage>,
        tools: &[ToolDefinition],
    ) -> CompletionRequestBuilder<M> {
        self.model
            .completion_request(prompt)
            .preamble(self.preamble.clone())
            .messages(history)
            .tools(tools.to_vec())
            .max_tokens_opt(self.max_tokens)
            .additional_params_opt(self.additional_params.clone())
    }
}

#[async_trait]
impl<M> LlmClient for RigLlm<M>
where
    M: CompletionModel + 'static,
    M::Client: Send + Sync + 'static,
{
    async fn complete(&self, session: &Session) -> anyhow::Result<String> {
        // Tool-less by contract: this is the single-shot path for aux callers
        // (reviewer / recall screening / briefing fallback), and it advertises no
        // tools at all — nothing here would dispatch a call the model made, so it
        // must not be able to ask for one. One completion is the whole answer.
        let (preamble, prompt, history) = self.assemble(session).await?;
        let turn = self.model_for(preamble, session);
        let (choice, _, _) = with_timeout(
            self.timeout,
            with_retry(|| {
                complete_once(
                    &turn,
                    &[],
                    RigMessage::user(prompt.clone()),
                    history.clone(),
                    self.stream,
                )
            }),
        )
        .await?;
        Ok(choice_text(&choice))
    }

    async fn begin_turn(&self, session: &Session) -> anyhow::Result<Box<dyn TurnDriver>> {
        let (preamble, prompt, history) = self.assemble(session).await?;
        Ok(Box::new(RigTurnDriver {
            turn: self.model_for(preamble, session),
            tools: self.tools.clone(),
            history,
            pending: Some(RigMessage::user(prompt)),
            stream: self.stream,
            timeout: self.timeout,
            usage: TokenUsage::default(),
        }))
    }
}

/// A [`TurnDriver`] over a per-turn [`TurnModel`]. Holds the growing conversation
/// history (excluding the not-yet-sent prompt) so each round is a single provider
/// completion — rig does one round-trip, komo owns the loop.
struct RigTurnDriver<M: CompletionModel> {
    turn: TurnModel<M>,
    /// Tool schemas re-sent every round (see [`RigLlm::tools`]).
    tools: Vec<ToolDefinition>,
    history: Vec<RigMessage>,
    /// The opening prompt; consumed by `first()`, then `None`.
    pending: Option<RigMessage>,
    /// Stream each round instead of one-shot `send()` (see [`RigLlm::stream`]).
    stream: bool,
    /// Per-round completion timeout (see [`RigLlm::timeout`]).
    timeout: Option<Duration>,
    /// Tokens spent so far this turn, summed over rounds; read by the runtime for
    /// the ledger once the turn ends.
    usage: TokenUsage,
}

impl<M> RigTurnDriver<M>
where
    M: CompletionModel + 'static,
{
    /// Send one round-trip: complete over `history + prompt`, then commit the
    /// assistant turn (verbatim — text + tool calls + reasoning together) to
    /// history so the next round sees a provider-correct transcript.
    ///
    /// The prompt is pushed onto `history` up front and split back off a single
    /// clone per attempt. A request builder takes prompt and history by value, so
    /// one clone of the round's messages is unavoidable; what this avoids is
    /// cloning the prompt *separately* from the history it will join, and it keeps
    /// every retry attempt reading one source of truth for the round's transcript.
    async fn run(&mut self, prompt: RigMessage) -> anyhow::Result<Step> {
        self.history.push(prompt);
        let stream = self.stream;
        let turn = &self.turn;
        let tools = &self.tools;
        let history = &self.history;

        let (choice, message_id, usage) = with_timeout(
            self.timeout,
            with_retry(|| async move {
                let mut messages = history.clone();
                let prompt = messages
                    .pop()
                    .expect("history holds the prompt pushed just above");
                complete_once(turn, tools, prompt, messages, stream).await
            }),
        )
        .await?;

        self.usage.add(usage);
        self.history.push(RigMessage::Assistant {
            id: message_id,
            content: choice.clone(),
        });
        Ok(choice_to_step(&choice))
    }
}

#[async_trait]
impl<M> TurnDriver for RigTurnDriver<M>
where
    M: CompletionModel + 'static,
{
    async fn first(&mut self) -> anyhow::Result<Step> {
        let prompt = self.pending.take().context("turn driver already started")?;
        self.run(prompt).await
    }

    async fn step(&mut self, results: Vec<ToolOutcome>) -> anyhow::Result<Step> {
        // One user message carrying every tool result, mirroring rig's own
        // `tool_result_user_content`: key by `call_id` when present (OpenAI),
        // else `id` (Anthropic).
        let contents: Vec<UserContent> = results
            .into_iter()
            .map(|r| {
                // A komo tool's model-facing result is plain text by contract
                // (`domain::tool::ToolOutput::text`), so it goes over as one text
                // block — no sniffing the payload for an image/multipart envelope.
                let content = OneOrMany::one(ToolResultContent::text(r.content));
                match r.call_id {
                    Some(call_id) => UserContent::tool_result_with_call_id(r.id, call_id, content),
                    None => UserContent::tool_result(r.id, content),
                }
            })
            .collect();
        let content = OneOrMany::many(contents)
            .map_err(|_| anyhow::anyhow!("no tool results to send back"))?;
        self.run(RigMessage::User { content }).await
    }

    fn usage(&self) -> TokenUsage {
        self.usage
    }
}

/// rig's per-response usage in komo's ledger units. A provider that reports
/// nothing yields zeros, which the ledger already reads as *unknown*.
fn token_usage(usage: &Usage) -> TokenUsage {
    TokenUsage {
        input: usage.input_tokens as i64,
        output: usage.output_tokens as i64,
    }
}

/// One provider round-trip, returning the assistant turn as
/// `(choice, message_id, usage)`.
///
/// `stream` picks the transport, not the semantics: backends that require
/// streaming (Codex) get their deltas aggregated back into the same triple the
/// one-shot `send()` yields, so every caller downstream is identical either way.
async fn complete_once<M>(
    turn: &TurnModel<M>,
    tools: &[ToolDefinition],
    prompt: RigMessage,
    history: Vec<RigMessage>,
    stream: bool,
) -> anyhow::Result<(OneOrMany<AssistantContent>, Option<String>, TokenUsage)>
where
    M: CompletionModel + 'static,
{
    let request = turn.request(prompt, history, tools);
    if !stream {
        let resp = request.send().await.context("LLM completion failed")?;
        return Ok((resp.choice, resp.message_id, token_usage(&resp.usage)));
    }
    // rig accumulates the streamed deltas into `choice`/`message_id` as the inner
    // stream drains, so we consume every chunk (surfacing any provider error) and
    // then read the final aggregate.
    let mut stream = request.stream().await.context("LLM completion failed")?;
    while let Some(item) = stream.next().await {
        item.context("LLM completion failed")?;
    }
    // Usage rides on the provider's final response frame, which not every
    // provider sends — absent means unknown, same as zeros.
    let usage = stream
        .response
        .as_ref()
        .map(|r| token_usage(&r.token_usage()))
        .unwrap_or_default();
    Ok((stream.choice.clone(), stream.message_id.clone(), usage))
}

/// Concatenate the text blocks of an assistant turn (ignoring tool calls /
/// reasoning) — the final answer for a tool-less completion.
fn choice_text(choice: &OneOrMany<AssistantContent>) -> String {
    let mut text = String::new();
    for content in choice.iter() {
        if let AssistantContent::Text(t) = content {
            text.push_str(&t.text);
        }
    }
    text
}

/// Split a model's assistant turn into komo's [`Step`]: any tool call makes it
/// a [`Step::ToolCalls`]; otherwise the concatenated text is the final answer.
/// Reasoning/image blocks are ignored for control flow (the driver still echoes
/// them back into history verbatim).
///
/// Text found *alongside* tool calls travels with them rather than being dropped:
/// it is the model narrating what it is about to do, which is the only account of
/// its reasoning a watcher gets (komo has no token streaming) and the honest thing
/// to fall back on if the round budget ends the turn early.
fn choice_to_step(choice: &OneOrMany<AssistantContent>) -> Step {
    let mut calls = Vec::new();
    let mut text = String::new();
    for content in choice.iter() {
        match content {
            AssistantContent::ToolCall(tc) => calls.push(ToolCallReq {
                id: tc.id.clone(),
                call_id: tc.call_id.clone(),
                name: tc.function.name.clone(),
                args: tc.function.arguments.to_string(),
            }),
            AssistantContent::Text(t) => text.push_str(&t.text),
            _ => {}
        }
    }
    if calls.is_empty() {
        Step::Final(text)
    } else {
        Step::ToolCalls { calls, text }
    }
}

/// Build an LLM client covering every provider the configured `models` menu
/// spans, exposing `tools` via function calling.
///
/// With a single-provider menu this is exactly one backend. With a
/// cross-provider one it is a [`RoutingLlm`] over one backend per provider, and
/// a session's qualified model id (`deepseek:deepseek-chat`) selects among them —
/// so switching provider is the same mechanism as switching model, decided per
/// turn off the session.
///
/// `preamble` is a factory (see [`PreambleFn`]) invoked once per turn to
/// (re)assemble the system prompt — typically wrapping a
/// [`crate::agent::system_prompt::SystemPromptBuilder`]. `enricher` is the
/// optional per-turn memory enrichment — `Some` only for the main agent, `None`
/// for aux/delegate sub-agents (they must not be fed the user's memory library).
pub fn build_llm(
    config: &ModelConfig,
    tools: Option<&crate::services::tool_execution::ToolExecutor>,
    preamble: PreambleFn,
    enricher: Option<Arc<MemoryEnricher>>,
) -> anyhow::Result<Arc<dyn LlmClient>> {
    let providers = config.menu_providers();
    // The common case: everything on the menu runs on one provider, so there is
    // nothing to route between.
    if providers.len() < 2 {
        return build_provider_llm(config, tools, preamble, enricher);
    }

    let mut by_provider = Vec::with_capacity(providers.len());
    for provider in providers {
        // Each backend's own default model is the first menu entry naming it —
        // for the configured provider that is `model` itself (the resolver force-
        // includes it first), so the default backend keeps its exact identity.
        let default_model = config
            .menu()
            .into_iter()
            .find(|entry| entry.provider == provider)
            .map(|entry| entry.model)
            .unwrap_or_else(|| provider.default_model().to_string());
        let scoped = config.for_provider(provider, default_model);
        by_provider.push((
            provider,
            build_provider_llm(&scoped, tools, preamble.clone(), enricher.clone())?,
        ));
    }
    Ok(Arc::new(RoutingLlm {
        by_provider,
        default_provider: config.provider,
    }))
}

/// Build the backend for exactly one provider (the erased `RigLlm`).
fn build_provider_llm(
    config: &ModelConfig,
    tools: Option<&crate::services::tool_execution::ToolExecutor>,
    preamble: PreambleFn,
    enricher: Option<Arc<MemoryEnricher>>,
) -> anyhow::Result<Arc<dyn LlmClient>> {
    // A missing API key degrades instead of failing construction: a fresh
    // install (first Docker boot, pre-`komo init`) must still bring the
    // gateway up — channels serve, pairing works — while every LLM call
    // reports the fix. Config resolution records the matching warning.
    if config.provider.uses_api_key() && config.api_key.is_empty() {
        return Ok(Arc::new(UnconfiguredLlm {
            message: format!(
                "{} is not set (required for {:?}). Add it to ~/.komo/.env \
                 (run `komo init` to scaffold one) or the container \
                 environment, then restart the gateway.",
                config.provider.api_key_var(),
                config.provider
            ),
        }));
    }
    // Only the schemas cross to the provider: the executor stays the single
    // dispatcher, so there is exactly one execution semantics (retry/ledger/cap)
    // for every tool call, and rig is never in a position to run one.
    let tool_defs: Vec<ToolDefinition> = tools
        .map(|executor| {
            executor
                .definitions()
                .into_iter()
                .map(|t| ToolDefinition {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    parameters: t.parameters_schema(),
                })
                .collect()
        })
        .unwrap_or_default();
    let model = config.model.clone();
    let key = config.api_key.clone();
    let base = config.base_url.as_deref();
    let max_history_messages = config.max_history_messages;
    let max_history_bytes = config.max_history_bytes;
    // The ChatGPT Codex backend only accepts streamed requests; everyone else
    // uses the simpler one-shot path. Declared before `rig_llm!` so the macro's
    // (hygienic) body can capture it alongside `preamble`/`enricher`.
    let stream = matches!(config.provider, Provider::Codex);
    // Cap each completion so a hung provider request fails the turn instead of
    // wedging it in `running` (rig's client sets no request timeout). `0` = off.
    let timeout =
        (config.llm_timeout_secs > 0).then(|| Duration::from_secs(config.llm_timeout_secs));

    // Each provider's client/model type differs (erased to `Arc<dyn LlmClient>`
    // at the end), so the five arms can't share a value — but minting the model
    // handle and wrapping it in `RigLlm` are identical. This macro factors that
    // tail out; only one arm runs, so moving `tool_defs`/`preamble`/`enricher`
    // per arm is fine. `client` is the only thing that varies.
    macro_rules! rig_llm {
        ($client:expr) => {{
            // Retained alongside the handle so a per-session model override can
            // mint a fresh one for the turn (`RigLlm::model_for`).
            let client = Arc::new($client);
            let handle = client.completion_model(model.clone());
            Arc::new(RigLlm {
                model: handle,
                client,
                tools: tool_defs,
                default_model: model,
                provider: config.provider,
                preamble,
                max_history_messages,
                max_history_bytes,
                enricher,
                stream,
                timeout,
            }) as Arc<dyn LlmClient>
        }};
    }

    let llm: Arc<dyn LlmClient> = match config.provider {
        Provider::DeepSeek => {
            let client = with_base_url(deepseek::Client::builder().api_key(key), base)
                .build()
                .context("failed to build DeepSeek client")?;
            rig_llm!(client)
        }
        Provider::OpenAi => {
            let client = with_base_url(openai::Client::builder().api_key(key), base)
                .build()
                .context("failed to build OpenAI client")?;
            rig_llm!(client)
        }
        Provider::Anthropic => {
            let client = with_base_url(anthropic::Client::builder().api_key(key), base)
                .build()
                .context("failed to build Anthropic client")?;
            rig_llm!(client)
        }
        Provider::OpenRouter => {
            let client = with_base_url(openrouter::Client::builder().api_key(key), base)
                .build()
                .context("failed to build OpenRouter client")?;
            rig_llm!(client)
        }
        Provider::Codex => {
            // Codex speaks the OpenAI Responses API (rig's default `openai`
            // client) but at the ChatGPT backend, authenticated with the Codex
            // CLI's OAuth tokens. `CodexHttpClient` re-stamps a fresh bearer on
            // every request; the static Cloudflare-dodging headers are baked in
            // here. `base` (config base_url) overrides the endpoint if set.
            //
            // Missing/broken credentials degrade like a missing API key: the
            // gateway must boot (a fresh box, or a container without
            // ~/.codex mounted) instead of crash-looping, with every LLM call
            // reporting the fix as the turn's reply.
            let auth = match CodexAuth::load() {
                Ok(auth) => auth,
                Err(error) => {
                    tracing::warn!(%error, "Codex credentials unavailable; LLM degraded");
                    return Ok(Arc::new(UnconfiguredLlm {
                        message: format!(
                            "Codex credentials unavailable: {error:#}. Run `codex` to log \
                             in (it writes ~/.codex/auth.json; $CODEX_HOME honored), then \
                             restart the gateway."
                        ),
                    }));
                }
            };
            let client = openai::Client::builder()
                .api_key(auth.initial_access_token())
                .base_url(base.unwrap_or(CODEX_BASE_URL))
                .http_headers(codex_static_headers(auth.account_id()))
                .http_client(CodexHttpClient::new(auth))
                .build()
                .context("failed to build Codex client")?;
            rig_llm!(client)
        }
    };
    Ok(llm)
}

/// Apply an optional base-URL override to any provider's client builder.
fn with_base_url<Ext, A, H>(
    builder: ClientBuilder<Ext, A, H>,
    base_url: Option<&str>,
) -> ClientBuilder<Ext, A, H>
where
    Ext: Clone,
{
    match base_url {
        Some(url) => builder.base_url(url),
        None => builder,
    }
}

/// Trim `prior` (the transcript before this turn's prompt) to the slice replayed
/// as model history, under two independent bounds.
///
/// Without a window, a long-lived chat session — telegram/feishu/wechat are keyed
/// by chat id and only rotate on an explicit `/new` — resends its whole transcript
/// every turn. `max_messages` is the count bound (`0` = keep everything);
/// `max_bytes` is the size bound (`0` = no size limit), and it exists because a
/// count says nothing about volume: twenty messages of pasted build output
/// overflow a context that two hundred chat lines sit inside. Both trim from the
/// oldest end, so the stable system prompt and memory prefix are untouched and the
/// upstream prompt cache is unaffected.
fn window_history(prior: &[Message], max_messages: usize, max_bytes: usize) -> &[Message] {
    let mut window = match max_messages {
        0 => prior,
        n => &prior[prior.len().saturating_sub(n)..],
    };
    if max_bytes > 0 {
        let size = |m: &Message| m.content.len() + m.tool_note.len();
        let mut total: usize = window.iter().map(size).sum();
        let mut start = 0;
        // A single message over the whole budget still gets dropped (the loop runs
        // to the end): sending it would blow the context on its own, and the turn's
        // own prompt is never part of this slice, so the model is not left mute.
        while start < window.len() && total > max_bytes {
            total -= size(&window[start]);
            start += 1;
        }
        window = &window[start..];
    }
    // The transcript strictly alternates user/assistant, so either cut can open on
    // an assistant message; drop it so history starts on a user turn (Anthropic
    // rejects a leading assistant message). Applied after both bounds, since
    // either one can be the cut that lands there.
    if window.first().is_some_and(|m| m.role == Role::Assistant) {
        window = &window[1..];
    }
    window
}

/// How many of the most recent assistant turns carry their tool-activity note
/// (`Message::tool_note`) into the model's history. Older notes are dropped:
/// "which file did I just read" decays in usefulness far faster than it decays in
/// context cost, and the run ledger keeps the full record either way.
const TOOL_NOTE_TURNS: usize = 3;

/// Index in `window` from which assistant messages may carry their tool note —
/// the start of the last [`TOOL_NOTE_TURNS`] note-bearing turns (`window.len()`
/// when there are none, i.e. attach nothing).
fn tool_note_cutoff(window: &[Message]) -> usize {
    let mut cutoff = window.len();
    let mut found = 0;
    for (idx, msg) in window.iter().enumerate().rev() {
        if msg.role == Role::Assistant && !msg.tool_note.is_empty() {
            cutoff = idx;
            found += 1;
            if found == TOOL_NOTE_TURNS {
                break;
            }
        }
    }
    cutoff
}

/// Map a komo message into a rig chat-history message. The system prompt is
/// supplied via the preamble, and tool outputs are folded into the following
/// assistant reply, so both `System` and `Tool` roles are skipped here.
///
/// `with_note` appends the turn's tool-activity digest to an assistant message,
/// which is what lets the *next* turn know tools ran at all — the user-visible
/// `content` stays exactly what every client renders.
fn to_rig_message(msg: &Message, with_note: bool) -> Option<RigMessage> {
    match msg.role {
        Role::User => Some(RigMessage::user(msg.content.clone())),
        Role::Assistant if with_note && !msg.tool_note.is_empty() => Some(RigMessage::assistant(
            format!("{}\n\n{}", msg.content, msg.tool_note),
        )),
        Role::Assistant => Some(RigMessage::assistant(msg.content.clone())),
        Role::System | Role::Tool => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_style_providers_send_reasoning_effort() {
        for provider in [Provider::OpenAi, Provider::OpenRouter, Provider::Codex] {
            assert_eq!(
                reasoning_params(provider, "high"),
                Some(json!({ "reasoning": { "effort": "high" } })),
                "{provider:?} should carry reasoning.effort"
            );
        }
    }

    #[test]
    fn anthropic_maps_effort_onto_a_thinking_budget() {
        let low = reasoning_params(Provider::Anthropic, "low").unwrap();
        let high = reasoning_params(Provider::Anthropic, "high").unwrap();
        let budget = |v: &Value| v["thinking"]["budget_tokens"].as_u64().unwrap();
        assert_eq!(low["thinking"]["type"], "enabled");
        assert!(
            budget(&low) < budget(&high),
            "a higher effort must buy more thinking"
        );
    }

    #[test]
    fn deepseek_and_unknown_levels_change_nothing() {
        // DeepSeek exposes no effort scale (`Provider::efforts` is empty), so a
        // level arriving anyway must not invent request params.
        assert_eq!(reasoning_params(Provider::DeepSeek, "high"), None);
        for level in ["", "  ", "auto", "xhigh", "HIGH"] {
            assert_eq!(reasoning_params(Provider::OpenAi, level), None, "{level:?}");
        }
    }

    #[test]
    fn every_advertised_effort_level_actually_maps() {
        // The menu a client is shown (`Provider::efforts`) and what reaches the
        // wire must agree — otherwise the UI offers a switch that does nothing.
        for provider in Provider::ALL {
            for level in provider.efforts() {
                assert!(
                    reasoning_params(provider, level).is_some(),
                    "{provider:?} advertises `{level}` but sends nothing"
                );
            }
        }
    }

    /// A backend that reports which provider it was routed to.
    struct Tagged(&'static str);

    #[async_trait]
    impl LlmClient for Tagged {
        async fn complete(&self, _session: &Session) -> anyhow::Result<String> {
            Ok(self.0.to_string())
        }
    }

    fn router() -> RoutingLlm {
        RoutingLlm {
            by_provider: vec![
                (
                    Provider::Codex,
                    Arc::new(Tagged("codex")) as Arc<dyn LlmClient>,
                ),
                (
                    Provider::DeepSeek,
                    Arc::new(Tagged("deepseek")) as Arc<dyn LlmClient>,
                ),
            ],
            default_provider: Provider::Codex,
        }
    }

    fn session_on(model: &str) -> Session {
        let mut session = Session::new("s");
        session.model = model.to_string();
        session
    }

    #[tokio::test]
    async fn a_qualified_id_routes_to_that_provider() {
        let router = router();
        assert_eq!(
            router
                .complete(&session_on("deepseek:deepseek-chat"))
                .await
                .unwrap(),
            "deepseek"
        );
    }

    #[tokio::test]
    async fn an_unqualified_or_default_id_stays_on_the_default_provider() {
        let router = router();
        for model in ["", "gpt-5.5", "codex:gpt-5.6-sol"] {
            assert_eq!(
                router.complete(&session_on(model)).await.unwrap(),
                "codex",
                "model {model:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_provider_with_no_backend_falls_back_to_the_default() {
        // Config can change under a stored session (a key removed, an entry
        // dropped), and running on the default beats failing the turn.
        let router = router();
        assert_eq!(
            router
                .complete(&session_on("anthropic:claude-sonnet-4-5"))
                .await
                .unwrap(),
            "codex"
        );
    }

    fn turn(user: &str, assistant: &str, note: &str) -> Vec<Message> {
        vec![
            Message::user(user),
            Message::assistant(assistant).with_tool_note(note),
        ]
    }

    #[test]
    fn the_byte_budget_trims_where_the_count_window_cannot() {
        // Two turns, the first carrying a pasted log. Both fit the count window,
        // so only the byte bound can keep the big one out — the case the count
        // window was blind to.
        let mut prior = turn("here is the log", &"x".repeat(5_000), "");
        prior.extend(turn("and now?", "short answer", ""));

        let counted = window_history(&prior, 50, 0);
        assert_eq!(counted.len(), 4, "the count window keeps everything");

        let bounded = window_history(&prior, 50, 1_000);
        assert_eq!(
            bounded.iter().map(|m| &m.content).collect::<Vec<_>>(),
            vec!["and now?", "short answer"],
            "the oversized turn is trimmed from the oldest end"
        );
    }

    #[test]
    fn a_window_never_opens_on_an_assistant_message() {
        // Whichever bound makes the cut, a leading assistant message must go:
        // Anthropic rejects one outright.
        let mut prior = turn("q1", "a1", "");
        prior.extend(turn("q2", "a2", ""));

        for window in [
            window_history(&prior, 3, 0), // count cut lands on "a1"
            window_history(&prior, 0, 5), // byte cut lands on "a1"
        ] {
            assert_eq!(
                window.first().map(|m| m.role.clone()),
                Some(Role::User),
                "history must start on a user turn, got {:?}",
                window.first().map(|m| &m.content)
            );
        }
    }

    #[test]
    fn the_byte_budget_is_off_at_zero_and_counts_tool_notes() {
        let prior = turn("q", "a", &"n".repeat(5_000));
        assert_eq!(window_history(&prior, 0, 0).len(), 2, "0 = unlimited");
        // The note is real context sent to the model, so it has to be weighed.
        assert!(
            window_history(&prior, 0, 1_000).is_empty(),
            "a note over the budget must be trimmed like content"
        );
    }

    #[test]
    fn only_the_most_recent_tool_notes_ride_along() {
        // Five note-bearing turns; the model should see the last TOOL_NOTE_TURNS.
        let mut prior = Vec::new();
        for i in 0..5 {
            prior.extend(turn(
                &format!("q{i}"),
                &format!("a{i}"),
                &format!("note{i}"),
            ));
        }
        let cutoff = tool_note_cutoff(&prior);
        let rendered: Vec<String> = prior
            .iter()
            .enumerate()
            .filter_map(|(idx, m)| to_rig_message(m, idx >= cutoff))
            .map(|m| format!("{m:?}"))
            .collect();
        let carried = |note: &str| rendered.iter().any(|m| m.contains(note));

        for recent in ["note2", "note3", "note4"] {
            assert!(carried(recent), "{recent} should still be carried");
        }
        for stale in ["note0", "note1"] {
            assert!(!carried(stale), "{stale} should have aged out");
        }
    }

    #[test]
    fn a_tool_note_never_touches_the_user_visible_content() {
        let msg = Message::assistant("the answer").with_tool_note("[tools used] read foo.rs");
        // Carried: the model sees both. Not carried: exactly the reply.
        let with = format!("{:?}", to_rig_message(&msg, true).unwrap());
        assert!(with.contains("the answer") && with.contains("read foo.rs"));
        let without = format!("{:?}", to_rig_message(&msg, false).unwrap());
        assert!(without.contains("the answer") && !without.contains("read foo.rs"));
        // And the stored message itself is untouched — every client renders this.
        assert_eq!(msg.content, "the answer");
    }

    /// The point of #1: one 429 on a late round must not throw the turn away.
    #[tokio::test(start_paused = true)]
    async fn a_transient_completion_failure_is_retried() {
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let result = with_retry(|| async {
            let n = attempts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n < 2 {
                anyhow::bail!("HTTP 429 Too Many Requests");
            }
            Ok("answered")
        })
        .await
        .unwrap();
        assert_eq!(result, "answered");
        assert_eq!(attempts.load(std::sync::atomic::Ordering::Relaxed), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn a_terminal_completion_failure_is_not_retried() {
        // An auth or schema error will fail identically forever; retrying it just
        // delays the message the user needs to see.
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let error = with_retry(|| async {
            attempts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            anyhow::bail!("invalid api key") as anyhow::Result<()>
        })
        .await
        .expect_err("terminal errors surface");
        assert!(format!("{error:#}").contains("invalid api key"));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn completion_retries_are_bounded() {
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let _ = with_retry(|| async {
            attempts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            anyhow::bail!("connection refused") as anyhow::Result<()>
        })
        .await
        .expect_err("a permanently down provider still fails");
        assert_eq!(
            attempts.load(std::sync::atomic::Ordering::Relaxed),
            LLM_RETRY_MAX_ATTEMPTS
        );
    }

    /// The retry budget lives *inside* the timeout, so a flapping provider can't
    /// multiply a turn's worst-case latency by the attempt count.
    #[tokio::test(start_paused = true)]
    async fn the_timeout_bounds_every_retry_together() {
        let started = tokio::time::Instant::now();
        let error = with_timeout(
            Some(Duration::from_secs(1)),
            with_retry(|| async {
                tokio::time::sleep(Duration::from_secs(10)).await;
                anyhow::bail!("connection refused") as anyhow::Result<()>
            }),
        )
        .await
        .expect_err("the round times out");
        assert!(format!("{error:#}").contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn text_alongside_tool_calls_survives_the_step_split() {
        use rig::completion::message::{ToolCall, ToolFunction};
        let choice = OneOrMany::many(vec![
            AssistantContent::text("Let me check the config first."),
            AssistantContent::ToolCall(ToolCall::new(
                "call-1".into(),
                ToolFunction {
                    name: "read".into(),
                    arguments: json!({ "path": "config.toml" }),
                },
            )),
        ])
        .unwrap();

        match choice_to_step(&choice) {
            Step::ToolCalls { calls, text } => {
                assert_eq!(calls.len(), 1);
                assert_eq!(text, "Let me check the config first.");
            }
            Step::Final(_) => panic!("a tool call must not read as a final answer"),
        }
    }

    #[test]
    fn merging_params_keeps_unrelated_keys_and_overrides_collisions() {
        let merged = merge_params(
            Some(json!({ "store": false, "reasoning": { "effort": "low" } })),
            json!({ "reasoning": { "effort": "high" } }),
        );
        assert_eq!(merged["store"], false);
        assert_eq!(merged["reasoning"]["effort"], "high");
        // No prior params, or a non-object one, is simply replaced.
        assert_eq!(merge_params(None, json!({ "a": 1 })), json!({ "a": 1 }));
        assert_eq!(
            merge_params(Some(Value::Null), json!({ "a": 1 })),
            json!({ "a": 1 })
        );
    }
}
