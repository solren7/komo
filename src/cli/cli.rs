use clap::{Parser, Subcommand};

use super::{chat, gateway, inspect, logs, memory, model, pair, service, wechat};

#[derive(Parser)]
#[command(name = "shion", version, about = "Personal agent framework")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start an interactive chat session
    Chat,
    /// Run the always-on gateway: maintenance scheduler (and, later,
    /// config-declared ingress channels). Maintenance cron comes from
    /// `schedule` in ~/.shion/config.toml (or SHION_SCHEDULE); default hourly.
    /// With no action, runs in the foreground (this is what launchd invokes).
    Gateway {
        #[command(subcommand)]
        action: Option<GatewayAction>,
    },
    /// Inspect scheduled reminders (recurring crons and one-shots)
    Cron {
        #[command(subcommand)]
        action: CronAction,
    },
    /// Inspect stored chat sessions
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
    /// Inspect the durable task list
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
    /// Inspect the run ledger (every agent turn and its tool steps)
    Run {
        #[command(subcommand)]
        action: RunAction,
    },
    /// Inspect and govern the long-term memory library
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
    /// Manage channel pairing: unknown senders must be approved from this
    /// host before the agent talks to them
    Pair {
        #[command(subcommand)]
        action: PairAction,
    },
    /// Show or switch the active LLM provider and model
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
    /// WeChat (微信) channel operator commands
    Wechat {
        #[command(subcommand)]
        action: WechatAction,
    },
    /// Print the gateway log (the launchd-captured tracing output)
    Logs {
        /// Number of trailing lines to print
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
        /// Follow the log, streaming new lines until Ctrl-C
        #[arg(short, long)]
        follow: bool,
        /// Show the stdout log (`gateway.log`) instead of the tracing log
        #[arg(long)]
        stdout: bool,
    },
    /// Print the shion version
    Version,
}

#[derive(Subcommand)]
enum WechatAction {
    /// Provision iLink credentials by scanning a login QR (run on the host)
    Login,
}

#[derive(Subcommand)]
enum ModelAction {
    /// Show the current provider/model and list available providers
    List,
    /// Switch provider (and optionally model); persists to config.toml
    Set {
        /// Provider: deepseek | openai | anthropic | openrouter
        provider: String,
        /// Model id (defaults to the provider's default model)
        model: Option<String>,
    },
}

#[derive(Subcommand)]
enum CronAction {
    /// List pending reminders with their schedules and next fire times
    List,
}

#[derive(Subcommand)]
enum TaskAction {
    /// List open tasks (inbox / todo / waiting), grouped by status
    List,
}

#[derive(Subcommand)]
enum RunAction {
    /// List recent runs (most recent first)
    List {
        /// Maximum number of runs to show
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show one run in full, including every tool step
    Inspect {
        /// Run id (as shown by `run list`)
        id: String,
    },
}

#[derive(Subcommand)]
enum MemoryAction {
    /// List stored memories (optionally filter by status), grouped by status
    List {
        /// Only show this status: candidate | active | archived | rejected
        #[arg(long)]
        status: Option<String>,
    },
    /// Substring search across all memories
    Search {
        /// Text to match in memory content
        query: String,
    },
    /// Promote a candidate to an active, confirmed memory
    Promote {
        /// Memory id (as shown by `memory list`)
        id: String,
    },
    /// Reject a candidate so it never surfaces
    Reject {
        /// Memory id
        id: String,
    },
    /// Pin a memory into the L1 per-turn profile (the manual, explicit path)
    Pin {
        /// Memory id
        id: String,
    },
}

#[derive(Subcommand)]
enum PairAction {
    /// List pending pairing requests (with codes) and approved senders
    List,
    /// Approve a pending request by its pairing code
    Approve {
        /// The code the bot sent to the unpaired chat
        code: String,
    },
    /// Remove a pairing by id (`platform:sender_id`, as shown by `pair list`)
    Revoke {
        /// Pairing id to remove
        id: String,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    /// List stored sessions with creation time and message counts
    List,
    /// Delete sessions that contain no messages
    Clean,
}

#[derive(Subcommand)]
enum GatewayAction {
    /// Install and start the gateway under launchd (auto-restart on crash,
    /// start at login)
    Start,
    /// Stop the gateway and remove it from launchd
    Stop,
    /// Restart the gateway under launchd (regenerates the plist, so a
    /// reinstalled binary is picked up)
    Restart,
    /// Show launchd state for the gateway
    Status,
}

pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    // The database always lives in the config directory; use SHION_HOME to
    // point at a different home (e.g. for tests or a second instance).
    let db = crate::config::default_db_url();
    // Durable tasks live in a separate file so resetting `shion.db` (disposable
    // dev state) never wipes them.
    let kanban = crate::config::default_kanban_db_url();
    match cli.command {
        Commands::Chat => chat::run(&db, &kanban).await,
        Commands::Gateway { action } => match action {
            None => {
                let schedule = crate::config::maintenance_schedule();
                gateway::run(&db, &kanban, &schedule).await
            }
            Some(GatewayAction::Start) => service::start(),
            Some(GatewayAction::Stop) => service::stop(),
            Some(GatewayAction::Restart) => service::restart(),
            Some(GatewayAction::Status) => service::status(),
        },
        Commands::Cron { action } => match action {
            CronAction::List => inspect::cron_list(&db).await,
        },
        Commands::Session { action } => match action {
            SessionAction::List => inspect::session_list(&db).await,
            SessionAction::Clean => inspect::session_clean(&db).await,
        },
        Commands::Task { action } => match action {
            TaskAction::List => inspect::task_list(&kanban).await,
        },
        Commands::Run { action } => match action {
            RunAction::List { limit } => inspect::run_list(&db, limit).await,
            RunAction::Inspect { id } => inspect::run_inspect(&db, &id).await,
        },
        Commands::Memory { action } => {
            let url = crate::config::default_memory_db_url();
            match action {
                MemoryAction::List { status } => memory::list(&url, status).await,
                MemoryAction::Search { query } => memory::search(&url, &query).await,
                MemoryAction::Promote { id } => memory::promote(&url, &id).await,
                MemoryAction::Reject { id } => memory::reject(&url, &id).await,
                MemoryAction::Pin { id } => memory::pin(&url, &id).await,
            }
        }
        Commands::Pair { action } => match action {
            PairAction::List => pair::list(&db).await,
            PairAction::Approve { code } => pair::approve(&db, &code).await,
            PairAction::Revoke { id } => pair::revoke(&db, &id).await,
        },
        Commands::Model { action } => match action {
            ModelAction::List => model::list(),
            ModelAction::Set { provider, model } => model::set(&provider, model),
        },
        Commands::Wechat { action } => match action {
            WechatAction::Login => wechat::login().await,
        },
        Commands::Logs {
            lines,
            follow,
            stdout,
        } => logs::run(lines, follow, stdout),
        Commands::Version => {
            println!("shion {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}
