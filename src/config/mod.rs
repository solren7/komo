//! Configuration as one resolved snapshot.
//!
//! Raw sources (`~/.komo/config.toml`, `KOMO_*` env vars, `.env` secrets) are
//! read once by [`sources::ConfigSources`] and resolved purely into a
//! [`ConfigSnapshot`]: the [`RuntimeConfig`] every caller consumes plus a
//! redacted [`ConfigReport`] of issues and provenance. Precedence (built-in
//! defaults < `config.toml` < `KOMO_*`), credential-missing semantics, and
//! per-value defaults live in `resolved.rs` — callers never re-derive them.
//!
//! Resolution never aborts: problems are recorded as [`ConfigIssue`]s so
//! diagnostics (`komo doctor`) always see the whole picture, while startup
//! paths fail fast via [`ConfigSnapshot::validate_agent`] /
//! [`ConfigSnapshot::validate_gateway`].

mod report;
mod resolved;
mod sources;
mod write;

use std::path::PathBuf;

pub use report::*;
pub use resolved::*;
pub use sources::ConfigSources;
pub use write::{
    validate_channel_config, write_channel_config, write_env_values, write_model_selection,
};

/// Supported LLM providers (all OpenAI-compatible or natively wired in `rig`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    DeepSeek,
    OpenAi,
    Anthropic,
    OpenRouter,
    /// OpenAI Codex via the ChatGPT backend, authenticated with the Codex CLI's
    /// OAuth tokens (`~/.codex/auth.json`) rather than an API key. See
    /// `infra/codex.rs`.
    Codex,
}

impl Provider {
    /// Every supported provider, in display order.
    pub const ALL: [Provider; 5] = [
        Provider::DeepSeek,
        Provider::OpenAi,
        Provider::Anthropic,
        Provider::OpenRouter,
        Provider::Codex,
    ];

    pub fn parse(s: &str) -> anyhow::Result<Self> {
        Ok(match s.trim().to_lowercase().as_str() {
            "deepseek" | "ds" => Provider::DeepSeek,
            "openai" | "oai" | "gpt" => Provider::OpenAi,
            "anthropic" | "claude" => Provider::Anthropic,
            "openrouter" | "or" => Provider::OpenRouter,
            "codex" | "openai-codex" => Provider::Codex,
            other => anyhow::bail!(
                "unknown provider `{other}` \
                 (expected: deepseek | openai | anthropic | openrouter | codex)"
            ),
        })
    }

    /// Canonical lowercase name, as written into `config.toml`.
    pub fn name(self) -> &'static str {
        match self {
            Provider::DeepSeek => "deepseek",
            Provider::OpenAi => "openai",
            Provider::Anthropic => "anthropic",
            Provider::OpenRouter => "openrouter",
            Provider::Codex => "codex",
        }
    }

    /// Default model id when `model` is unset.
    pub fn default_model(self) -> &'static str {
        match self {
            Provider::DeepSeek => "deepseek-chat",
            Provider::OpenAi => "gpt-4o-mini",
            Provider::Anthropic => "claude-3-5-sonnet-latest",
            Provider::OpenRouter => "deepseek/deepseek-chat",
            // A slug the ChatGPT Codex backend currently accepts (others seen:
            // gpt-5.4, gpt-5.4-mini). Account-/tier-dependent — override via
            // config.toml `model`; discover live at GET /codex/models.
            Provider::Codex => "gpt-5.5",
        }
    }

    /// Environment variable holding this provider's API key. Codex has none —
    /// it authenticates from `~/.codex/auth.json` (see [`Provider::uses_api_key`]).
    pub fn api_key_var(self) -> &'static str {
        match self {
            Provider::DeepSeek => "DEEPSEEK_API_KEY",
            Provider::OpenAi => "OPENAI_API_KEY",
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::OpenRouter => "OPENROUTER_API_KEY",
            Provider::Codex => "",
        }
    }

    /// Whether this provider authenticates with an environment API key.
    /// Codex is the exception: its credentials come from the Codex CLI's OAuth
    /// login, resolved at build time in `infra/codex.rs`.
    pub fn uses_api_key(self) -> bool {
        !matches!(self, Provider::Codex)
    }
}

/// One resolved view of everything komo is configured to do, plus the
/// redacted diagnostics that explain it. Load once per process (or construct
/// from explicit [`ConfigSources`] in tests) and pass it down — callers never
/// re-read `config.toml`, the env, or `.env`.
pub struct ConfigSnapshot {
    pub runtime: RuntimeConfig,
    pub report: ConfigReport,
}

impl ConfigSnapshot {
    /// Read all sources once (ensuring `~/.komo` exists) and resolve.
    /// Never fails — problems land in the report; validate before starting
    /// long-running work.
    pub fn load() -> Self {
        Self::from_sources(ConfigSources::load(ensure_komo_home()))
    }

    /// Pure resolution seam: tests provide sources directly instead of
    /// mutating the real process environment or filesystem.
    pub fn from_sources(sources: ConfigSources) -> Self {
        let (runtime, report) = resolved::resolve(sources);
        Self { runtime, report }
    }

    /// Fail on the issues that make an agent turn impossible: a malformed
    /// `KOMO_*` env or an unusable model selection. Channel problems don't
    /// block a chat turn — the gateway checks those via [`Self::validate_gateway`].
    pub fn validate_agent(&self) -> anyhow::Result<()> {
        Self::ok_or(
            self.report
                .fatal_matching(|i| i.path == "env" || i.path.starts_with("model")),
        )
    }

    /// Fail on *any* fatal issue — the gateway hosts every surface, so an
    /// enabled-but-misconfigured channel must stop startup, matching the old
    /// per-resolver fail-fast behavior.
    pub fn validate_gateway(&self) -> anyhow::Result<()> {
        Self::ok_or(self.report.fatal())
    }

    fn ok_or(fatal: Option<&ConfigIssue>) -> anyhow::Result<()> {
        match fatal {
            Some(issue) => Err(anyhow::anyhow!("{}", issue.message)),
            None => Ok(()),
        }
    }
}

// `komo_home` / `ensure_komo_home` moved to `komo-core` (the dependency-light
// crate the GUI client shares) so both resolve the same `~/.komo` without
// depending on komo's runtime. Re-exported here so `config::komo_home()` /
// `config::ensure_komo_home()` call sites are unchanged.
pub use komo_core::paths::{ensure_komo_home, komo_home};

/// Directory holding the cached Chinese workday calendar, one `{year}.json` per
/// year: `<komo_home>/workdays/`. Disposable — delete a file to force a
/// re-fetch from the holiday API.
pub fn workday_cache_dir() -> PathBuf {
    komo_home().join("workdays")
}

/// Where the WeChat QR-login credentials are stored. Shared by the gateway
/// channel and the `komo channel wechat login` provisioning command.
pub fn wechat_cred_path() -> PathBuf {
    komo_home().join("wechat").join("credentials.json")
}

// `komo_home` / `default_home` tests moved to `komo_core::paths` with the code.
