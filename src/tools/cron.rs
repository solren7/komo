//! The `cron` tool: scheduled jobs, managed from inside a conversation.
//!
//! `komo cron …` is the operator's surface for the same store; this is the
//! agent's. Both go through the shared operator actions
//! (`services::operator_control::actions`), so validation, name uniqueness and
//! the initial `next_run_at` can't fork between "the user typed a command" and
//! "the user asked in chat".
//!
//! Trust boundary. A CLI-authored job is operator-authored by construction —
//! whoever ran `komo cron add` already had shell on the host. A chat-authored
//! job is *model*-authored, so every mutation is gated through the `Approver`:
//!
//! - **agent mode** (a prompt) is `Risk::Normal`, scope `cron:add` — the turn it
//!   schedules still runs unattended, where side effects pass only through an
//!   `unattended = true` `[policy]` rule.
//! - **command mode** (a program) is `Risk::Dangerous` and carries an
//!   `ActionRef::Shell`, so a `[policy]` deny rule fences it and no ordinary
//!   shell *allow* rule can silently grant it (`include_dangerous` is required).
//!   It runs directly, unattended, with no approver at fire time — the operator
//!   is approving every future execution at once, so the prompt says so.
//! - remove/enable/disable/run are `Risk::Normal`, scope `cron:manage`.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::{
        approval::{ActionRef, ApprovalRequest, Approver},
        cron::{
            CronAction, CronJob, CronJobRepository, CronJobSpec, CronRunStatus,
            DEFAULT_CRON_JOB_TIMEOUT_SECS,
        },
        tool::Tool,
    },
    services::operator_control::actions,
};

/// How much of an agent job's prompt a listing shows.
const PROMPT_PREVIEW: usize = 100;

#[derive(Deserialize)]
struct CronArgs {
    action: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    schedule: Option<String>,
    // Agent-mode fields.
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    skills: Vec<String>,
    // Command-mode fields.
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// Lets the model create and manage the gateway's scheduled jobs (`cron.db`) —
/// the recurring-work counterpart to `reminder`, which only re-delivers a
/// message.
pub struct CronTool {
    jobs: Arc<dyn CronJobRepository>,
    approver: Arc<dyn Approver>,
}

impl CronTool {
    pub fn new(jobs: Arc<dyn CronJobRepository>, approver: Arc<dyn Approver>) -> Self {
        Self { jobs, approver }
    }

    /// Gate one management mutation (everything but `add`, which describes its
    /// own action). One scope key for the family: approving "manage my jobs"
    /// once per session shouldn't re-prompt per job.
    async fn approve_manage(&self, summary: String) -> bool {
        self.approver
            .approve(&ApprovalRequest::normal(summary).with_scope_key("cron:manage".to_string()))
            .await
    }
}

#[async_trait]
impl Tool for CronTool {
    fn name(&self) -> &'static str {
        "cron"
    }

    fn description(&self) -> &'static str {
        "Manage the gateway's scheduled jobs — recurring *work*, unlike \
         `reminder`, which only re-delivers a message. \
         action=\"list\" returns every job with its schedule, next run and last \
         outcome; \
         action=\"add\" creates one (requires `name` + a 5-field `schedule` in \
         the user's local timezone, plus either `prompt` for an agent job — an \
         unattended agent turn with your full tool set, optionally preloading \
         `skills` — or `command` (+ `args`/`workdir`/`timeout_secs`) for a fixed \
         program); \
         action=\"disable\" / \"enable\" pauses and resumes a job by `name`; \
         action=\"remove\" deletes it; action=\"run\" fires it once now. \
         Jobs fire only while `komo gateway` runs, and each run's output is \
         delivered to the user's home channel, not into this conversation. \
         Creating or changing a job asks the user for approval. Use this for \
         \"every morning summarize X\" / \"每周五跑一下这个脚本\"; use `reminder` \
         for a plain nudge and `task` for one-off work with no clock."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "add", "remove", "enable", "disable", "run"],
                    "description": "The job operation."
                },
                "name": {
                    "type": "string",
                    "description": "Job name — the unique key every action but `list` takes. Short and descriptive (e.g. \"morning-brief\"); no whitespace or `:` `/` `\\`."
                },
                "schedule": {
                    "type": "string",
                    "description": "5-field cron expression in the user's local timezone, e.g. \"0 8 * * *\" for 8 AM daily or \"0 14 * * 5\" for Friday 2 PM (action=add)."
                },
                "prompt": {
                    "type": "string",
                    "description": "The instruction an agent-mode job runs each time it fires. Write it as a self-contained task — the turn has no conversation history (action=add; pick prompt OR command)."
                },
                "skills": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Skills the agent job loads before acting (action=add, agent mode)."
                },
                "command": {
                    "type": "string",
                    "description": "Absolute path of the program a command-mode job runs (no shell, so no pipes/globs). Needs prominent approval — prefer an agent job unless the user named a script (action=add; pick prompt OR command)."
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Arguments for `command` (action=add, command mode)."
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory for `command` (action=add, command mode)."
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Wall-clock budget for `command`; the process is killed past it (action=add, command mode; default 900)."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: String) -> anyhow::Result<String> {
        let args: CronArgs = serde_json::from_str(&input)
            .map_err(|e| anyhow::anyhow!("invalid cron arguments: {e}"))?;
        let now = time::OffsetDateTime::now_utc().unix_timestamp();

        match args.action.as_str() {
            "list" => {
                let jobs = self.jobs.list().await?;
                if jobs.is_empty() {
                    return Ok("No scheduled jobs.".to_string());
                }
                Ok(jobs.iter().map(describe_job).collect::<Vec<_>>().join("\n"))
            }

            "add" => {
                let name = require_name(&args.name)?;
                let schedule = args
                    .schedule
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "`schedule` is required for action=add — a 5-field cron \
                             expression like \"0 8 * * *\" (local time)"
                        )
                    })?
                    .to_string();

                let (action, request) = match (args.prompt, args.command) {
                    (Some(_), Some(_)) => anyhow::bail!(
                        "pass either `prompt` (agent job) or `command` (program job), not both"
                    ),
                    (Some(prompt), None) => {
                        let request = ApprovalRequest::normal(format!(
                            "Schedule agent job `{name}` [{schedule}]: {}",
                            oneline(&prompt, PROMPT_PREVIEW)
                        ))
                        .with_scope_key("cron:add".to_string());
                        (
                            CronAction::Agent {
                                prompt,
                                skills: args.skills,
                            },
                            request,
                        )
                    }
                    (None, Some(command)) => {
                        let line = command_line(&command, &args.args);
                        // Approving this approves every future execution: the
                        // sweep runs a command job directly, with no approver.
                        let request = ApprovalRequest::dangerous(
                            format!("Schedule command job `{name}` [{schedule}]: {line}"),
                            format!(
                                "The gateway will run `{line}` unattended on this schedule, \
                                 with no further approval each time. Remove it with \
                                 `komo cron remove {name}`."
                            ),
                        )
                        .with_action(ActionRef::Shell { command: line });
                        (
                            CronAction::Command {
                                command,
                                args: args.args,
                                workdir: args.workdir,
                                timeout_secs: args
                                    .timeout_secs
                                    .unwrap_or(DEFAULT_CRON_JOB_TIMEOUT_SECS),
                            },
                            request,
                        )
                    }
                    (None, None) => anyhow::bail!(
                        "action=add needs either `prompt` (an agent job) or `command` \
                         (a fixed program)"
                    ),
                };

                if !self.approver.approve(&request).await {
                    return Ok(format!(
                        "Job `{name}` rejected by user; nothing was scheduled."
                    ));
                }

                // Shared with `komo cron add` and the api channel: schedule
                // parsing, name rules and uniqueness live in one place.
                let job = actions::add_cron_job(
                    self.jobs.as_ref(),
                    CronJobSpec {
                        name,
                        schedule,
                        action,
                    },
                    now,
                )
                .await?;
                Ok(format!(
                    "Scheduled {} job `{}` [{}] — first run {}. Runs while `komo gateway` \
                     is up; output goes to the home channel.",
                    job.action.kind(),
                    job.name,
                    job.schedule,
                    local_time(job.next_run_at)
                ))
            }

            "remove" => {
                let name = require_name(&args.name)?;
                // Confirm it exists before prompting: "approve deleting a job
                // that isn't there" is a pointless question.
                if self.jobs.find_by_name(&name).await?.is_none() {
                    anyhow::bail!("{}", actions::no_cron_job_message(&name));
                }
                if !self
                    .approve_manage(format!("Delete scheduled job `{name}`"))
                    .await
                {
                    return Ok(format!("Rejected by user; job `{name}` was kept."));
                }
                if !self.jobs.delete(&name).await? {
                    anyhow::bail!("{}", actions::no_cron_job_message(&name));
                }
                Ok(format!("Removed job `{name}`."))
            }

            action @ ("enable" | "disable") => {
                let name = require_name(&args.name)?;
                let enabled = action == "enable";
                let verb = if enabled { "Resume" } else { "Pause" };
                if self.jobs.find_by_name(&name).await?.is_none() {
                    anyhow::bail!("{}", actions::no_cron_job_message(&name));
                }
                if !self
                    .approve_manage(format!("{verb} scheduled job `{name}`"))
                    .await
                {
                    return Ok(format!(
                        "Rejected by user; job `{name}` was left as it was."
                    ));
                }
                let job = actions::set_cron_enabled(self.jobs.as_ref(), &name, enabled, now)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("{}", actions::no_cron_job_message(&name)))?;
                Ok(if enabled {
                    format!(
                        "Enabled job `{}` — next run {}.",
                        job.name,
                        local_time(job.next_run_at)
                    )
                } else {
                    format!(
                        "Disabled job `{}`. It stays listed and can be re-enabled.",
                        job.name
                    )
                })
            }

            "run" => {
                let name = require_name(&args.name)?;
                if self.jobs.find_by_name(&name).await?.is_none() {
                    anyhow::bail!("{}", actions::no_cron_job_message(&name));
                }
                if !self
                    .approve_manage(format!("Run scheduled job `{name}` now"))
                    .await
                {
                    return Ok(format!("Rejected by user; job `{name}` was not run."));
                }
                let job = actions::trigger_cron_job(self.jobs.as_ref(), &name, now)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("{}", actions::no_cron_job_message(&name)))?;
                Ok(format!(
                    "Job `{}` is due now — the gateway runs it on its next sweep tick \
                     (within a minute) and delivers the output to the home channel.",
                    job.name
                ))
            }

            other => Err(anyhow::anyhow!(
                "unknown action `{other}` (expected list/add/remove/enable/disable/run)"
            )),
        }
    }
}

fn require_name(name: &Option<String>) -> anyhow::Result<String> {
    let name = name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .ok_or_else(|| anyhow::anyhow!("`name` is required for this action"))?;
    Ok(name.to_string())
}

/// One job as a line the model can relay: name, kind, schedule, state, target,
/// and the last outcome when there is one.
fn describe_job(job: &CronJob) -> String {
    let state = if job.enabled {
        format!("next {}", local_time(job.next_run_at))
    } else {
        "disabled".to_string()
    };
    let target = match &job.action {
        CronAction::Command { command, args, .. } => command_line(command, args),
        CronAction::Agent { prompt, skills } => {
            let skills = if skills.is_empty() {
                String::new()
            } else {
                format!(" [skills: {}]", skills.join(", "))
            };
            format!("{}{skills}", oneline(prompt, PROMPT_PREVIEW))
        }
    };
    let mut line = format!(
        "{} ({}) [{}] {} → {}",
        job.name,
        job.action.kind(),
        job.schedule,
        state,
        target
    );
    if let (Some(at), Some(status)) = (job.last_run_at, &job.last_status) {
        line.push_str(&format!(
            " | last run {} {}",
            local_time(at),
            status.as_str()
        ));
        if *status == CronRunStatus::Failed && !job.last_error.is_empty() {
            line.push_str(&format!(" — {}", oneline(&job.last_error, PROMPT_PREVIEW)));
        }
    }
    line
}

fn command_line(command: &str, args: &[String]) -> String {
    std::iter::once(command)
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Collapse to one line and cap at `max` characters (never mid-char).
fn oneline(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    format!("{}…", flat.chars().take(max).collect::<String>())
}

fn local_time(unix: i64) -> String {
    chrono::DateTime::from_timestamp(unix, 0)
        .map(|dt| dt.with_timezone(&chrono::Local).to_rfc3339())
        .unwrap_or_else(|| unix.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeJobs {
        jobs: Mutex<Vec<CronJob>>,
    }

    #[async_trait]
    impl CronJobRepository for FakeJobs {
        async fn save(&self, job: &CronJob) -> anyhow::Result<()> {
            self.jobs.lock().unwrap().push(job.clone());
            Ok(())
        }
        async fn list(&self) -> anyhow::Result<Vec<CronJob>> {
            Ok(self.jobs.lock().unwrap().clone())
        }
        async fn find_by_name(&self, name: &str) -> anyhow::Result<Option<CronJob>> {
            Ok(self
                .jobs
                .lock()
                .unwrap()
                .iter()
                .find(|j| j.name == name)
                .cloned())
        }
        async fn update(&self, job: &CronJob) -> anyhow::Result<()> {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(slot) = jobs.iter_mut().find(|j| j.id == job.id) {
                *slot = job.clone();
            }
            Ok(())
        }
        async fn delete(&self, name: &str) -> anyhow::Result<bool> {
            let mut jobs = self.jobs.lock().unwrap();
            let before = jobs.len();
            jobs.retain(|j| j.name != name);
            Ok(jobs.len() != before)
        }
    }

    /// Records what it was asked and answers with a fixed verdict.
    struct Recorder {
        allow: bool,
        seen: Mutex<Vec<(String, crate::domain::approval::Risk)>>,
    }

    impl Recorder {
        fn new(allow: bool) -> Arc<Self> {
            Arc::new(Self {
                allow,
                seen: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl Approver for Recorder {
        async fn approve(&self, request: &ApprovalRequest) -> bool {
            self.seen
                .lock()
                .unwrap()
                .push((request.summary.clone(), request.risk));
            self.allow
        }
    }

    fn tool(allow: bool) -> (CronTool, Arc<FakeJobs>, Arc<Recorder>) {
        let jobs = Arc::new(FakeJobs::default());
        let approver = Recorder::new(allow);
        let t = CronTool::new(
            jobs.clone() as Arc<dyn CronJobRepository>,
            approver.clone() as Arc<dyn Approver>,
        );
        (t, jobs, approver)
    }

    #[tokio::test]
    async fn add_agent_job_persists_after_approval() {
        let (t, jobs, approver) = tool(true);
        let out = t
            .execute(
                json!({"action": "add", "name": "morning-brief", "schedule": "0 8 * * *",
                       "prompt": "总结我今天的日程", "skills": ["calendar"]})
                .to_string(),
            )
            .await
            .unwrap();
        assert!(out.contains("morning-brief"), "{out}");
        assert!(out.contains("first run"), "{out}");

        let stored = jobs.jobs.lock().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].schedule, "0 8 * * *");
        let CronAction::Agent { prompt, skills } = &stored[0].action else {
            panic!("agent job");
        };
        assert_eq!(prompt, "总结我今天的日程");
        assert_eq!(skills, &vec!["calendar".to_string()]);
        assert!(stored[0].next_run_at > 0, "schedule was resolved");

        let seen = approver.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].1, crate::domain::approval::Risk::Normal);
    }

    #[tokio::test]
    async fn command_job_is_gated_as_dangerous() {
        let (t, jobs, approver) = tool(true);
        t.execute(
            json!({"action": "add", "name": "rotate", "schedule": "0 14 * * 5",
                   "command": "/opt/rotate.py", "args": ["--push"]})
            .to_string(),
        )
        .await
        .unwrap();
        let seen = approver.seen.lock().unwrap();
        assert_eq!(seen[0].1, crate::domain::approval::Risk::Dangerous);
        assert!(seen[0].0.contains("/opt/rotate.py --push"), "{}", seen[0].0);
        assert_eq!(jobs.jobs.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn denied_add_stores_nothing() {
        let (t, jobs, _) = tool(false);
        let out = t
            .execute(
                json!({"action": "add", "name": "x", "schedule": "0 8 * * *", "prompt": "hi"})
                    .to_string(),
            )
            .await
            .unwrap();
        assert!(out.contains("rejected"), "{out}");
        assert!(jobs.jobs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn add_rejects_bad_schedule_and_missing_action_fields() {
        let (t, jobs, _) = tool(true);
        // A schedule croner can't parse never reaches the store.
        assert!(
            t.execute(
                json!({"action": "add", "name": "x", "schedule": "nope", "prompt": "hi"})
                    .to_string(),
            )
            .await
            .is_err()
        );
        // Neither prompt nor command.
        assert!(
            t.execute(json!({"action": "add", "name": "x", "schedule": "0 8 * * *"}).to_string())
                .await
                .is_err()
        );
        // Both.
        assert!(
            t.execute(
                json!({"action": "add", "name": "x", "schedule": "0 8 * * *",
                       "prompt": "hi", "command": "/bin/true"})
                .to_string(),
            )
            .await
            .is_err()
        );
        // No schedule at all.
        assert!(
            t.execute(json!({"action": "add", "name": "x", "prompt": "hi"}).to_string())
                .await
                .is_err()
        );
        assert!(jobs.jobs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn add_rejects_a_name_that_is_not_key_shaped() {
        let (t, jobs, _) = tool(true);
        let err = t
            .execute(
                json!({"action": "add", "name": "morning brief", "schedule": "0 8 * * *",
                       "prompt": "hi"})
                .to_string(),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid job name"), "{err}");
        assert!(jobs.jobs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn add_rejects_duplicate_name() {
        let (t, _, _) = tool(true);
        let add = json!({"action": "add", "name": "dup", "schedule": "0 8 * * *", "prompt": "hi"})
            .to_string();
        t.execute(add.clone()).await.unwrap();
        let err = t.execute(add).await.unwrap_err().to_string();
        assert!(err.contains("already exists"), "{err}");
    }

    #[tokio::test]
    async fn disable_then_enable_recomputes_next_run() {
        let (t, jobs, _) = tool(true);
        t.execute(
            json!({"action": "add", "name": "j", "schedule": "0 8 * * *", "prompt": "hi"})
                .to_string(),
        )
        .await
        .unwrap();

        let out = t
            .execute(json!({"action": "disable", "name": "j"}).to_string())
            .await
            .unwrap();
        assert!(out.contains("Disabled"), "{out}");
        assert!(!jobs.jobs.lock().unwrap()[0].enabled);

        let out = t
            .execute(json!({"action": "enable", "name": "j"}).to_string())
            .await
            .unwrap();
        assert!(out.contains("next"), "{out}");
        let stored = jobs.jobs.lock().unwrap();
        assert!(stored[0].enabled);
        assert!(stored[0].next_run_at > time::OffsetDateTime::now_utc().unix_timestamp());
    }

    #[tokio::test]
    async fn run_makes_the_job_due_now() {
        let (t, jobs, _) = tool(true);
        t.execute(
            json!({"action": "add", "name": "j", "schedule": "0 8 * * *", "prompt": "hi"})
                .to_string(),
        )
        .await
        .unwrap();
        let out = t
            .execute(json!({"action": "run", "name": "j"}).to_string())
            .await
            .unwrap();
        assert!(out.contains("due now"), "{out}");
        assert!(
            jobs.jobs.lock().unwrap()[0].next_run_at
                <= time::OffsetDateTime::now_utc().unix_timestamp()
        );
    }

    #[tokio::test]
    async fn denied_management_leaves_the_job_alone() {
        let (t, jobs, _) = tool(true);
        t.execute(
            json!({"action": "add", "name": "j", "schedule": "0 8 * * *", "prompt": "hi"})
                .to_string(),
        )
        .await
        .unwrap();
        // A fresh tool over the same store, this time denying.
        let denier = CronTool::new(
            jobs.clone() as Arc<dyn CronJobRepository>,
            Recorder::new(false) as Arc<dyn Approver>,
        );
        let out = denier
            .execute(json!({"action": "remove", "name": "j"}).to_string())
            .await
            .unwrap();
        assert!(out.contains("kept"), "{out}");
        assert_eq!(jobs.jobs.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unknown_job_errors_without_prompting() {
        let (t, _, approver) = tool(true);
        for action in ["remove", "enable", "disable", "run"] {
            let err = t
                .execute(json!({"action": action, "name": "ghost"}).to_string())
                .await
                .unwrap_err()
                .to_string();
            assert!(err.contains("no cron job named"), "{action}: {err}");
        }
        assert!(
            approver.seen.lock().unwrap().is_empty(),
            "a missing job must not raise an approval prompt"
        );
    }

    #[tokio::test]
    async fn list_reports_schedule_state_and_last_outcome() {
        let (t, jobs, _) = tool(true);
        assert_eq!(
            t.execute(json!({"action": "list"}).to_string())
                .await
                .unwrap(),
            "No scheduled jobs."
        );
        t.execute(
            json!({"action": "add", "name": "j", "schedule": "0 8 * * *",
                   "prompt": "a very long prompt that goes on and on"})
            .to_string(),
        )
        .await
        .unwrap();
        {
            let mut stored = jobs.jobs.lock().unwrap();
            stored[0].last_run_at = Some(1_700_000_000);
            stored[0].last_status = Some(CronRunStatus::Failed);
            stored[0].last_error = "boom\nsecond line".into();
        }
        let out = t
            .execute(json!({"action": "list"}).to_string())
            .await
            .unwrap();
        assert!(out.contains("j (agent) [0 8 * * *]"), "{out}");
        assert!(out.contains("last run"), "{out}");
        assert!(out.contains("failed — boom second line"), "{out}");
    }

    #[tokio::test]
    async fn unknown_action_errors() {
        let (t, _, _) = tool(true);
        assert!(
            t.execute(json!({"action": "frobnicate"}).to_string())
                .await
                .is_err()
        );
    }

    #[test]
    fn oneline_flattens_and_caps_on_char_boundaries() {
        assert_eq!(oneline("a\n b  c", 40), "a b c");
        assert_eq!(oneline("日程日程日程", 3), "日程日…");
    }
}
