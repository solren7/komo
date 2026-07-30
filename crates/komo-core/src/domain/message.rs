use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub timestamp: i64,
    /// Model-facing footnote on an assistant message: what tools that turn ran
    /// (`domain::run::tool_digest`). Kept **beside** `content` rather than inside
    /// it because the two have different audiences — `content` is what the user
    /// reads in every client, this is what the next turn's model needs in order
    /// to know the turn used tools at all. Empty for user messages, for assistant
    /// turns that called no tools, and for rows written before the column existed.
    #[serde(default)]
    pub tool_note: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            timestamp: now(),
            tool_note: String::new(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            timestamp: now(),
            tool_note: String::new(),
        }
    }

    /// Attach the turn's tool digest (builder form, so the runtime can compose a
    /// finished assistant message in one expression).
    pub fn with_tool_note(mut self, note: impl Into<String>) -> Self {
        self.tool_note = note.into();
        self
    }
}

fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}
