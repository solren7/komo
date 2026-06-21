mod agent;
mod cli;
mod config;
mod domain;
mod infra;
mod services;
mod tools;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // cwd .env first (developer override), then ~/.shion/.env.
    // dotenvy never overwrites an already-set variable, so the first loader wins.
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_path(config::ensure_shion_home().join(".env"));
    init_tracing();
    cli::run().await
}

/// Install the tracing subscriber. Without this every `info!`/`warn!`/`debug!`
/// in the codebase is a no-op (events emitted, no consumer). Logs go to stderr
/// (launchd captures the gateway's via the plist's `StandardErrorPath`); the
/// level is controlled by `SHION_LOG` (e.g. `SHION_LOG=debug`), defaulting to
/// `info`. `try_init` so a second call (e.g. in tests) is a harmless no-op.
fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};
    // Default: shion's own logs at info, but mute two sources of noise —
    // toasty's per-connect schema chatter, and rig's `prompt_request` INFO
    // events, which log every tool call's *full result* verbatim (a wall of
    // text for any list-returning tool). shion's own `tool ok` span line
    // (name/seq/elapsed, no result) still records each call concisely.
    // `SHION_LOG` overrides the whole filter (e.g. `debug` to see everything).
    let filter = EnvFilter::try_from_env("SHION_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,toasty=warn,rig_core=warn"));
    let _ = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
