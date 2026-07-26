use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    agent::daemon::next_occurrence_local,
    domain::{
        context::ToolContext,
        reminder::{Reminder, ReminderRepository, ReminderStatus},
        tool::{Tool, ToolError, ToolOutput, parse_args},
    },
};

#[derive(Deserialize)]
struct ReminderArgs {
    action: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    after: Option<String>,
    #[serde(default)]
    at: Option<String>,
    #[serde(default)]
    cron: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

pub struct ReminderTool {
    reminders: Arc<dyn ReminderRepository>,
}

impl ReminderTool {
    pub fn new(reminders: Arc<dyn ReminderRepository>) -> Self {
        Self { reminders }
    }
}

#[async_trait]
impl Tool for ReminderTool {
    fn name(&self) -> &'static str {
        "reminder"
    }

    fn description(&self) -> &'static str {
        "Schedule reminders delivered as desktop notifications by the gateway \
         process. action=\"create\" schedules a new reminder (requires message + \
         one of after/at/cron); action=\"list\" returns pending reminders; \
         action=\"cancel\" cancels a reminder by id."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "list", "cancel"],
                    "description": "The reminder operation."
                },
                "message": {
                    "type": "string",
                    "description": "Reminder text to deliver (action=create)."
                },
                "after": {
                    "type": "string",
                    "description": "Relative delay: \"45s\", \"5m\", \"2h\", \"1d\" (action=create, pick one of after/at)."
                },
                "at": {
                    "type": "string",
                    "description": "Absolute RFC3339 fire time, e.g. \"2025-01-01T09:00:00+08:00\" (action=create, pick one of after/at/cron)."
                },
                "cron": {
                    "type": "string",
                    "description": "5-field cron expression in the user's local timezone, e.g. \"0 9 * * *\" for 9 AM daily (action=create, pick one of after/at/cron)."
                },
                "id": {
                    "type": "string",
                    "description": "Reminder id to cancel (action=cancel)."
                }
            },
            "required": ["action"]
        })
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let args: ReminderArgs = parse_args(&input)?;

        match args.action.as_str() {
            "create" => {
                let message = args.message.ok_or_else(|| {
                    ToolError::InvalidInput("`message` is required for action=create".to_string())
                })?;

                let now = time::OffsetDateTime::now_utc().unix_timestamp();

                match (args.after, args.at, args.cron) {
                    (Some(after), _, _) => {
                        let delay = parse_after(&after)
                            .map_err(|e| ToolError::InvalidInput(e.to_string()))?;
                        let run_at = now + delay.as_secs() as i64;
                        let reminder = Reminder::new(message, run_at);
                        let id = reminder.id.clone();
                        let fire_time = chrono::DateTime::from_timestamp(run_at, 0)
                            .map(|dt| dt.with_timezone(&chrono::Local).to_rfc3339())
                            .unwrap_or_else(|| run_at.to_string());
                        self.reminders.save(&reminder).await?;
                        Ok(ToolOutput::text(format!(
                            "Reminder {id} set for {fire_time}. \
                             Delivered by the gateway process — make sure `komo gateway` is running."
                        ))
                        .with_structured(json!({ "id": id, "run_at": run_at })))
                    }
                    (_, Some(at), _) => {
                        let dt = chrono::DateTime::parse_from_rfc3339(&at).map_err(|e| {
                            ToolError::InvalidInput(format!("invalid `at` time `{at}`: {e}"))
                        })?;
                        let run_at = dt.timestamp();
                        let reminder = Reminder::new(message, run_at);
                        let id = reminder.id.clone();
                        let fire_time = dt.with_timezone(&chrono::Local).to_rfc3339();
                        self.reminders.save(&reminder).await?;
                        Ok(ToolOutput::text(format!(
                            "Reminder {id} set for {fire_time}. \
                             Delivered by the gateway process — make sure `komo gateway` is running."
                        ))
                        .with_structured(json!({ "id": id, "run_at": run_at })))
                    }
                    (_, _, Some(cron)) => {
                        let run_at = next_occurrence_local(&cron, now).map_err(|e| {
                            ToolError::InvalidInput(format!("invalid `cron` expression: {e}"))
                        })?;
                        let reminder = Reminder::recurring(message, run_at, cron.clone());
                        let id = reminder.id.clone();
                        let next_time = chrono::DateTime::from_timestamp(run_at, 0)
                            .map(|dt| dt.with_timezone(&chrono::Local).to_rfc3339())
                            .unwrap_or_else(|| run_at.to_string());
                        self.reminders.save(&reminder).await?;
                        Ok(ToolOutput::text(format!(
                            "Recurring reminder {id} set: {cron} (next at {next_time}). \
                             Delivered by the gateway process — make sure `komo gateway` is running."
                        ))
                        .with_structured(
                            json!({ "id": id, "run_at": run_at, "schedule": cron }),
                        ))
                    }
                    (None, None, None) => Err(ToolError::InvalidInput(
                        "one of `after`, `at`, or `cron` is required for action=create".to_string(),
                    )),
                }
            }

            "list" => {
                let pending = self.reminders.list_pending().await?;
                if pending.is_empty() {
                    return Ok(ToolOutput::text("No pending reminders."));
                }
                let lines: Vec<String> = pending
                    .iter()
                    .map(|r| {
                        let fire_time = chrono::DateTime::from_timestamp(r.run_at, 0)
                            .map(|dt| dt.with_timezone(&chrono::Local).to_rfc3339())
                            .unwrap_or_else(|| r.run_at.to_string());
                        if r.is_recurring() {
                            format!(
                                "{}: {} (due {}, repeats: {})",
                                r.id, r.message, fire_time, r.schedule
                            )
                        } else {
                            format!("{}: {} (due {})", r.id, r.message, fire_time)
                        }
                    })
                    .collect();
                Ok(ToolOutput::text(lines.join("\n"))
                    .with_title(format!("{} pending reminders", pending.len())))
            }

            "cancel" => {
                let id = args.id.ok_or_else(|| {
                    ToolError::InvalidInput("`id` is required for action=cancel".to_string())
                })?;
                self.reminders
                    .set_status(&id, ReminderStatus::Cancelled)
                    .await?;
                Ok(ToolOutput::text(format!("Reminder {id} cancelled.")))
            }

            other => Err(ToolError::InvalidInput(format!(
                "unknown action `{other}` (expected create/list/cancel)"
            ))),
        }
    }
}

/// Parse a relative duration string: `<number><unit>` where unit is s/m/h/d.
pub fn parse_after(s: &str) -> anyhow::Result<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return Err(anyhow::anyhow!("empty duration string"));
    }
    let (digits, unit) = s.split_at(s.len() - 1);
    let n: u64 = digits
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid duration `{s}`: expected format like `5m`"))?;
    let secs = match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86400,
        other => {
            return Err(anyhow::anyhow!(
                "unknown unit `{other}` in `{s}` (expected s/m/h/d)"
            ));
        }
    };
    Ok(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // ── parse_after ───────────────────────────────────────────────────────────

    #[test]
    fn parse_after_supports_s_m_h_d() {
        assert_eq!(parse_after("45s").unwrap(), Duration::from_secs(45));
        assert_eq!(parse_after("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_after("2h").unwrap(), Duration::from_secs(7200));
        assert_eq!(parse_after("1d").unwrap(), Duration::from_secs(86400));
    }

    #[test]
    fn parse_after_rejects_invalid() {
        assert!(parse_after("abc").is_err());
        assert!(parse_after("5x").is_err());
        assert!(parse_after("").is_err());
    }

    // ── FakeReminderRepository ────────────────────────────────────────────────

    #[derive(Default)]
    struct FakeRepo {
        reminders: Mutex<Vec<Reminder>>,
    }

    #[async_trait]
    impl ReminderRepository for FakeRepo {
        async fn save(&self, reminder: &Reminder) -> anyhow::Result<()> {
            self.reminders.lock().unwrap().push(reminder.clone());
            Ok(())
        }

        async fn list_pending(&self) -> anyhow::Result<Vec<Reminder>> {
            Ok(self
                .reminders
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.status == ReminderStatus::Pending)
                .cloned()
                .collect())
        }

        async fn set_status(&self, id: &str, status: ReminderStatus) -> anyhow::Result<()> {
            if let Some(r) = self
                .reminders
                .lock()
                .unwrap()
                .iter_mut()
                .find(|r| r.id == id)
            {
                r.status = status;
            }
            Ok(())
        }

        async fn reschedule(&self, id: &str, next_run_at: i64) -> anyhow::Result<()> {
            if let Some(r) = self
                .reminders
                .lock()
                .unwrap()
                .iter_mut()
                .find(|r| r.id == id)
            {
                r.run_at = next_run_at;
            }
            Ok(())
        }
    }

    fn tool() -> (ReminderTool, Arc<FakeRepo>) {
        let repo = Arc::new(FakeRepo::default());
        let t = ReminderTool::new(repo.clone() as Arc<dyn ReminderRepository>);
        (t, repo)
    }

    fn ctx() -> ToolContext {
        crate::tools::test_support::detached_ctx("cli:test")
    }

    #[tokio::test]
    async fn reminder_tool_create_persists_pending() {
        let (t, repo) = tool();
        let result = t
            .call(
                json!({"action": "create", "message": "drink water", "after": "1m"}),
                &ctx(),
            )
            .await
            .unwrap();
        assert!(result.text.contains("set for"));
        assert!(result.text.contains("gateway"));
        let pending = repo.reminders.lock().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].status, ReminderStatus::Pending);
        assert_eq!(pending[0].message, "drink water");
    }

    #[tokio::test]
    async fn reminder_tool_cancel_sets_status() {
        let (t, repo) = tool();
        t.call(
            json!({"action": "create", "message": "foo", "after": "5m"}),
            &ctx(),
        )
        .await
        .unwrap();
        let id = repo.reminders.lock().unwrap()[0].id.clone();
        t.call(json!({"action": "cancel", "id": id}), &ctx())
            .await
            .unwrap();
        assert_eq!(
            repo.reminders.lock().unwrap()[0].status,
            ReminderStatus::Cancelled
        );
    }

    #[tokio::test]
    async fn reminder_tool_create_with_cron_persists_schedule() {
        let (t, repo) = tool();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let result = t
            .call(
                json!({"action": "create", "message": "take medication", "cron": "0 9 * * *"}),
                &ctx(),
            )
            .await
            .unwrap();

        assert!(result.text.contains("Recurring"));
        assert!(result.text.contains("0 9 * * *"));

        let rems = repo.reminders.lock().unwrap();
        assert_eq!(rems.len(), 1);
        assert_eq!(rems[0].schedule, "0 9 * * *");
        assert!(rems[0].run_at > now);
        assert_eq!(rems[0].status, ReminderStatus::Pending);
    }

    #[tokio::test]
    async fn reminder_tool_rejects_invalid_cron() {
        let (t, repo) = tool();
        let result = t
            .call(
                json!({"action": "create", "message": "foo", "cron": "not a cron"}),
                &ctx(),
            )
            .await;

        // A bad cron is the model's mistake to fix, not a transient failure.
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
        assert!(repo.reminders.lock().unwrap().is_empty());
    }
}
