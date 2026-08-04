//! Cron-job mutations shared by every caller that creates or changes a job.
//!
//! The `cron` tool (in conversation), the gateway's `/api/cron/*` handlers and
//! the direct CLI adapter all funnel through these functions, so validation —
//! schedule parsing, name uniqueness, the initial `next_run_at` — cannot fork
//! between the paths. `OperatorActions` wraps them for the operator-control
//! surface; it does not reimplement them.

use komo_core::domain::cron::{
    CronAction, CronJob, CronJobRepository, CronJobSpec, DEFAULT_CRON_JOB_TIMEOUT_SECS,
    MAX_CRON_JOB_NAME_LEN, next_occurrence_local, valid_cron_job_name,
};

/// Validate a job spec and create it — schedule parsed with the same cron
/// parser the sweep uses (so nothing invalid ever reaches the store), name
/// uniqueness enforced, and the initial `next_run_at` computed from now.
/// Shared by the gateway's `/api/cron/add` handler and the direct adapter, so
/// validation can't fork between the two paths.
pub async fn add_cron_job(
    jobs: &dyn CronJobRepository,
    spec: CronJobSpec,
    now: i64,
) -> anyhow::Result<CronJob> {
    let name = spec.name.trim();
    if name.is_empty() {
        anyhow::bail!("a cron job needs a name");
    }
    // A name is a key (every `komo cron` subcommand, and an agent job's session
    // id) — keep it key-shaped. Matters most on the agent's `cron` tool path,
    // where the name is model-authored.
    if !valid_cron_job_name(name) {
        anyhow::bail!(
            "invalid job name `{name}`: no whitespace or `:` `/` `\\`, at most \
             {MAX_CRON_JOB_NAME_LEN} characters"
        );
    }
    // Normalize + validate the action per kind.
    let action = match spec.action {
        CronAction::Command {
            command,
            args,
            workdir,
            timeout_secs,
        } => {
            if command.trim().is_empty() {
                anyhow::bail!("a command cron job needs a command");
            }
            CronAction::Command {
                command: command.trim().to_string(),
                args,
                workdir: workdir.filter(|w| !w.trim().is_empty()),
                timeout_secs: if timeout_secs > 0 {
                    timeout_secs
                } else {
                    DEFAULT_CRON_JOB_TIMEOUT_SECS
                },
            }
        }
        CronAction::Agent { prompt, skills } => {
            if prompt.trim().is_empty() {
                anyhow::bail!("an agent cron job needs a prompt");
            }
            CronAction::Agent {
                prompt: prompt.trim().to_string(),
                skills: skills
                    .into_iter()
                    .filter(|s| !s.trim().is_empty())
                    .collect(),
            }
        }
    };
    if jobs.find_by_name(name).await?.is_some() {
        anyhow::bail!("a cron job named `{name}` already exists");
    }
    // Also proves the expression parses — next_occurrence_local rejects
    // anything croner can't schedule.
    let next_run_at = next_occurrence_local(&spec.schedule, now)?;
    let job = CronJob::new(name, &spec.schedule, action, next_run_at);
    jobs.save(&job).await?;
    Ok(job)
}

/// Flip a job's enabled flag; `None` = no such job. Re-enabling recomputes
/// `next_run_at` from now — a stale past slot must not fire the moment the job
/// comes back (a broken-schedule job that the sweep disabled keeps its stored
/// expression, so this also surfaces the parse error to the operator).
pub async fn set_cron_enabled(
    jobs: &dyn CronJobRepository,
    name: &str,
    enabled: bool,
    now: i64,
) -> anyhow::Result<Option<CronJob>> {
    let Some(mut job) = jobs.find_by_name(name).await? else {
        return Ok(None);
    };
    if enabled && !job.enabled {
        job.next_run_at = next_occurrence_local(&job.schedule, now)?;
    }
    job.enabled = enabled;
    jobs.update(&job).await?;
    Ok(Some(job))
}

/// Make a job due immediately (the sweep picks it up on its next tick);
/// `None` = no such job. The job must be enabled — triggering a disabled job
/// would silently do nothing until someone re-enabled it.
pub async fn trigger_cron_job(
    jobs: &dyn CronJobRepository,
    name: &str,
    now: i64,
) -> anyhow::Result<Option<CronJob>> {
    let Some(mut job) = jobs.find_by_name(name).await? else {
        return Ok(None);
    };
    if !job.enabled {
        anyhow::bail!("cron job `{name}` is disabled — enable it first (`komo cron enable`)");
    }
    job.next_run_at = now;
    jobs.update(&job).await?;
    Ok(Some(job))
}

/// One uniform unknown-job message (the gateway's 404 body and the direct
/// path's error must read identically).
pub fn no_cron_job_message(name: &str) -> String {
    format!("no cron job named `{name}`")
}
