use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use rig::{
    agent::Agent,
    client::{ClientBuilder, CompletionClient},
    completion::{CompletionModel, Message as RigMessage, Prompt},
    providers::{anthropic, deepseek, openai, openrouter},
    tool::ToolDyn,
};

use crate::{
    agent::system_prompt::{render_pinned_memory_block, render_recalled_memory_block},
    config::{ModelConfig, Provider},
    domain::{
        llm::LlmClient,
        memory::{MemoryContext, MemoryRepository},
        message::{Message, Role},
        session::Session,
        tool::Tool,
    },
    infra::rig_tool::RigTool,
};

/// Produces the system prompt (preamble) on demand. Called once per user turn
/// so the prompt is rebuilt per session rather than baked once at startup —
/// the gateway is a long-lived process, so a baked prompt would freeze the
/// volatile tier (date) at boot. The factory's output is day-precision, so it
/// stays byte-identical across turns within a day (upstream prompt cache stays
/// warm) and self-heals across midnight.
pub type PreambleFn = Arc<dyn Fn() -> String + Send + Sync>;

/// Max facts pulled per turn by L3 active recall. Small on purpose: recall is
/// background context, top-ranked relevance only. See `docs/memory-injection-plan.md`.
const RECALL_LIMIT: usize = 5;

/// Generic [`LlmClient`] over any `rig` completion model. The concrete provider
/// type is erased behind `Arc<dyn LlmClient>` by [`build_llm`].
pub struct RigLlm<M: CompletionModel> {
    agent: Agent<M>,
    /// Maximum tool-calling round-trips per user turn before the agent must
    /// answer (config `max_turns`, env `SHION_MAX_TURNS`).
    max_turns: usize,
    /// Rebuilds the system prompt each turn (see [`PreambleFn`]).
    preamble: PreambleFn,
    /// Optional long-term memory store. When set (the main agent), each turn's
    /// L1 pinned profile is injected after the preamble. `None` for aux/delegate
    /// sub-agents, which must not be fed the user's memory library.
    memories: Option<Arc<dyn MemoryRepository>>,
}

#[async_trait]
impl<M> LlmClient for RigLlm<M>
where
    M: CompletionModel + 'static,
{
    async fn complete(&self, session: &Session) -> anyhow::Result<String> {
        // The current prompt is the most recent user message; everything before
        // it forms the conversation history sent to the model.
        let last_user_idx = session
            .messages
            .iter()
            .rposition(|m| m.role == Role::User)
            .context("no user message to respond to")?;
        let prompt = session.messages[last_user_idx].content.clone();
        let history: Vec<RigMessage> = session.messages[..last_user_idx]
            .iter()
            .filter_map(to_rig_message)
            .collect();

        // Rebuild the system prompt for this turn. `Agent` is cheap to clone
        // (`Arc<model>` + an `Arc`-backed tool handle), and its `preamble` field
        // is public, so we clone-and-override rather than mutate shared state —
        // keeping concurrent sessions in the gateway independent.
        let mut preamble = (self.preamble)();

        // L1 pinned-memory injection: appended after the volatile tier so the
        // stable+context+volatile prefix stays byte-stable (pinned changes only
        // when the pinned set changes, far less than per turn). Failure is
        // non-fatal — memory is background context, it must not fail a reply —
        // but it is logged, or "why doesn't it know me today" is unanswerable.
        if let Some(memories) = &self.memories {
            let ctx = MemoryContext::from_session(&session.id);

            // L1 pinned profile. Capture the ids so the same memory is not also
            // echoed by L3 recall below (a pinned memory is active + in-scope, so
            // it would otherwise surface twice).
            let mut pinned_ids: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            match memories.pinned(&ctx).await {
                Ok(pinned) => {
                    pinned_ids = pinned.iter().map(|m| m.id.clone()).collect();
                    if let Some(block) = render_pinned_memory_block(&pinned) {
                        preamble.push_str("\n\n");
                        preamble.push_str(&block);
                    }
                }
                Err(error) => tracing::warn!(%error, "failed to load pinned memories"),
            }

            // L3 active recall: facts relevant to this turn's message, appended
            // after pinned so the volatile|pinned|recall order holds (pinned is
            // cross-turn stable, recall is per-query cold — stable goes first to
            // keep the prefix cacheable). Same non-fatal-but-logged contract.
            match memories.recall(&ctx, &prompt, RECALL_LIMIT).await {
                Ok(mut hits) => {
                    hits.retain(|h| !pinned_ids.contains(&h.memory.id));
                    if let Some(block) = render_recalled_memory_block(&hits) {
                        preamble.push_str("\n\n");
                        preamble.push_str(&block);
                    }
                    // Record the recall usage signal off the reply path: it only
                    // touches `last_used_at`, so it must not add latency or fail
                    // the answer. Spawned best-effort, warn on error.
                    let ids: Vec<String> = hits.iter().map(|h| h.memory.id.clone()).collect();
                    if !ids.is_empty() {
                        let repo = memories.clone();
                        tokio::spawn(async move {
                            let now = time::OffsetDateTime::now_utc().unix_timestamp();
                            if let Err(error) = repo.mark_used(&ids, now).await {
                                tracing::warn!(%error, "failed to record recall usage");
                            }
                        });
                    }
                }
                Err(error) => tracing::warn!(%error, "failed to recall memories"),
            }
        }

        let mut agent = self.agent.clone();
        agent.preamble = Some(preamble);

        let reply = agent
            .prompt(prompt)
            .with_history(history)
            .max_turns(self.max_turns)
            .await
            .context("LLM completion failed")?;
        Ok(reply)
    }
}

/// Build an LLM client for the configured provider, exposing `tools` via
/// function calling. `preamble` is a factory (see [`PreambleFn`]) invoked once
/// per turn to (re)assemble the system prompt — typically wrapping a
/// [`crate::agent::system_prompt::SystemPromptBuilder`]. The factory's initial
/// output is baked into the agent; each turn overrides it. The concrete
/// provider model type is erased. `memories` is the optional long-term store
/// for L1 pinned injection — `Some` for the main agent, `None` for aux/delegate
/// sub-agents.
pub fn build_llm(
    config: &ModelConfig,
    tools: Vec<Arc<dyn Tool>>,
    preamble: PreambleFn,
    memories: Option<Arc<dyn MemoryRepository>>,
) -> anyhow::Result<Arc<dyn LlmClient>> {
    let adapters: Vec<Box<dyn ToolDyn>> = tools
        .into_iter()
        .map(|t| Box::new(RigTool(t)) as Box<dyn ToolDyn>)
        .collect();
    let model = config.model.clone();
    let key = config.api_key.clone();
    let base = config.base_url.as_deref();
    let max_turns = config.max_turns;
    // Seed the agent with an initial preamble; `complete` overrides it per turn.
    let initial = preamble();

    // Each provider's client/agent type differs (erased to `Arc<dyn LlmClient>`
    // at the end), so the four arms can't share a value — but the agent-build
    // and `RigLlm` wrapping are identical. This macro factors that tail out;
    // only one arm runs, so moving `adapters`/`preamble`/`memories` per arm is
    // fine. `client` is the only thing that varies.
    macro_rules! rig_llm {
        ($client:expr) => {{
            let agent = $client
                .agent(model.clone())
                .preamble(&initial)
                .tools(adapters)
                .build();
            Arc::new(RigLlm {
                agent,
                max_turns,
                preamble,
                memories,
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

/// Map a shion message into a rig chat-history message. The system prompt is
/// supplied via the preamble, and tool outputs are folded into the following
/// assistant reply, so both `System` and `Tool` roles are skipped here.
fn to_rig_message(msg: &Message) -> Option<RigMessage> {
    match msg.role {
        Role::User => Some(RigMessage::user(msg.content.clone())),
        Role::Assistant => Some(RigMessage::assistant(msg.content.clone())),
        Role::System | Role::Tool => None,
    }
}
