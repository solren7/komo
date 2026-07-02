/// A named capability package: lightweight metadata (`name` + `description`)
/// plus a full instruction body loaded on demand (progressive disclosure).
///
/// Governance metadata lives in the same `SKILL.md` frontmatter as the
/// identity fields (skills are files — roadmap §9; the filesystem is the single
/// source of truth):
/// - `protected`: only the operator may change this skill — the reviewer never
///   writes a candidate proposal for it.
/// - `disabled`: kept on disk and inspectable, but hidden from the model's
///   catalog; `skill view` reports it as disabled instead of loading it.
/// - `source`: provenance — `user` (hand-written, the default) or `reviewer`
///   (extracted by the reflective reviewer).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub instructions: String,
    #[serde(default)]
    pub protected: bool,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default = "default_source")]
    pub source: String,
}

/// Provenance values for [`Skill::source`].
pub const SOURCE_USER: &str = "user";
pub const SOURCE_REVIEWER: &str = "reviewer";

fn default_source() -> String {
    SOURCE_USER.to_string()
}

/// A skill name doubles as its directory name on disk, so it must be a plain
/// path segment: non-empty, `[A-Za-z0-9._-]`, and not starting with `.` (dot
/// prefixes are reserved for governance dirs like `.candidates`). This is the
/// floor that keeps an LLM-suggested name from escaping the skills tree.
pub fn valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

impl Skill {
    /// Parse a `SKILL.md` document: YAML-ish frontmatter (`name`, `description`,
    /// and the governance keys `protected` / `disabled` / `source`) fenced by
    /// `---`, followed by the instruction body.
    pub fn parse(content: &str) -> Option<Skill> {
        let rest = content.trim_start().strip_prefix("---")?;
        let fence = rest.find("\n---")?;
        let front = &rest[..fence];
        let body = rest[fence + "\n---".len()..]
            .trim_start_matches('-')
            .trim_start_matches(['\n', '\r'])
            .trim()
            .to_string();

        let mut name = None;
        let mut description = None;
        let mut protected = false;
        let mut disabled = false;
        let mut source = default_source();
        for line in front.lines() {
            if let Some(v) = line.strip_prefix("name:") {
                name = Some(unquote(v.trim()));
            } else if let Some(v) = line.strip_prefix("description:") {
                description = Some(unquote(v.trim()));
            } else if let Some(v) = line.strip_prefix("protected:") {
                protected = v.trim() == "true";
            } else if let Some(v) = line.strip_prefix("disabled:") {
                disabled = v.trim() == "true";
            } else if let Some(v) = line.strip_prefix("source:") {
                source = unquote(v.trim());
            }
        }

        let name = name?;
        if name.is_empty() {
            return None;
        }
        Some(Skill {
            name,
            description: description.unwrap_or_default(),
            instructions: body,
            protected,
            disabled,
            source,
        })
    }
}

fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_and_body() {
        let doc = "---\nname: summarize-file\ndescription: \"Summarize a file\"\n---\n\nStep 1. Read it.\nStep 2. Summarize.\n";
        let skill = Skill::parse(doc).unwrap();
        assert_eq!(skill.name, "summarize-file");
        assert_eq!(skill.description, "Summarize a file");
        assert!(skill.instructions.starts_with("Step 1."));
        assert!(skill.instructions.contains("Step 2."));
        assert!(!skill.protected);
        assert!(!skill.disabled);
        assert_eq!(skill.source, SOURCE_USER);
    }

    #[test]
    fn parses_governance_keys() {
        let doc = "---\nname: risky\nprotected: true\ndisabled: true\nsource: reviewer\n---\nbody";
        let skill = Skill::parse(doc).unwrap();
        assert!(skill.protected);
        assert!(skill.disabled);
        assert_eq!(skill.source, SOURCE_REVIEWER);
    }

    #[test]
    fn rejects_document_without_frontmatter() {
        assert!(Skill::parse("no frontmatter here").is_none());
    }

    #[test]
    fn skill_names_must_be_plain_path_segments() {
        assert!(valid_skill_name("feishu-calendar"));
        assert!(valid_skill_name("v2_sync.beta"));
        assert!(!valid_skill_name(""));
        assert!(!valid_skill_name(".candidates"));
        assert!(!valid_skill_name("../escape"));
        assert!(!valid_skill_name("a/b"));
        assert!(!valid_skill_name("with space"));
    }
}
