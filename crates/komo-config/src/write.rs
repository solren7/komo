//! The one config *write* path: persisting a model selection.

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use super::Provider;

/// Persist the provider/model selection into `<home>/config.toml`, preserving
/// every other key already present (schedule, base_url, aux_model, …).
///
/// `model: None` removes the `model` key so the provider's default applies.
/// Returns the path written. Note: any `KOMO_PROVIDER` / `KOMO_MODEL` env
/// vars still take priority over the file at resolve time.
pub fn write_model_selection(
    home: &Path,
    provider: Provider,
    model: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let path = home.join("config.toml");
    let mut table: toml::Table = match std::fs::read_to_string(&path) {
        Ok(s) => toml::from_str(&s)
            .map_err(|e| anyhow::anyhow!("{} is invalid TOML: {e}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(e.into()),
    };

    table.insert(
        "provider".to_string(),
        toml::Value::String(provider.name().to_string()),
    );
    match model {
        Some(m) => {
            table.insert("model".to_string(), toml::Value::String(m.to_string()));
        }
        None => {
            table.remove("model");
        }
    }

    atomic_write(&path, &toml::to_string_pretty(&table)?, None)?;
    Ok(path)
}

/// Update named credentials in `<home>/.env`, preserving comments and every
/// unrelated line. A commented template entry (`# KEY=`) becomes active.
pub fn write_env_values(home: &Path, values: &[(&str, &str)]) -> anyhow::Result<PathBuf> {
    for (key, value) in values {
        if key.is_empty()
            || !key
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
        {
            anyhow::bail!("invalid environment variable name `{key}`");
        }
        if value.contains(['\n', '\r']) {
            anyhow::bail!("credential `{key}` must be one line");
        }
    }

    let path = home.join(".env");
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let mut pending = values
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut lines = Vec::new();
    let mut written = std::collections::BTreeSet::new();
    for line in content.lines() {
        let trimmed = line.trim_start();
        let candidate = trimmed
            .strip_prefix("# ")
            .unwrap_or(trimmed)
            .trim_start_matches('#');
        let replacement = values.iter().find_map(|(key, value)| {
            candidate
                .strip_prefix(key)
                .filter(|rest| rest.starts_with('='))
                .map(|_| ((*key).to_string(), env_assignment(key, value)))
        });
        if let Some((key, replacement)) = replacement {
            pending.remove(&key);
            if written.insert(key) {
                lines.push(replacement);
            }
        } else {
            lines.push(line.to_string());
        }
    }
    lines.extend(
        pending
            .into_iter()
            .map(|(key, value)| env_assignment(&key, &value)),
    );
    let output = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };
    atomic_write(&path, &output, Some(0o600))?;
    Ok(path)
}

/// Merge one `[channels.<name>]` table into config.toml without disturbing
/// unrelated runtime settings or channel tables.
pub fn write_channel_config(
    home: &Path,
    channel: &str,
    values: impl IntoIterator<Item = (&'static str, toml::Value)>,
) -> anyhow::Result<PathBuf> {
    let (path, body) = render_channel_config(home, channel, values)?;
    atomic_write(&path, &body, None)?;
    Ok(path)
}

/// Validate and serialize a channel-table merge without touching disk. Setup
/// uses this before changing credentials, so a malformed config cannot leave
/// a newly written secret with no corresponding channel configuration.
pub fn validate_channel_config(
    home: &Path,
    channel: &str,
    values: impl IntoIterator<Item = (&'static str, toml::Value)>,
) -> anyhow::Result<()> {
    let _ = render_channel_config(home, channel, values)?;
    Ok(())
}

fn render_channel_config(
    home: &Path,
    channel: &str,
    values: impl IntoIterator<Item = (&'static str, toml::Value)>,
) -> anyhow::Result<(PathBuf, String)> {
    if channel.is_empty()
        || !channel
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    {
        anyhow::bail!("invalid channel name `{channel}`");
    }
    let path = home.join("config.toml");
    let mut root: toml::Table = match std::fs::read_to_string(&path) {
        Ok(s) => toml::from_str(&s)
            .map_err(|e| anyhow::anyhow!("{} is invalid TOML: {e}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(e.into()),
    };
    let channels = root
        .entry("channels".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("{} has non-table `channels`", path.display()))?;
    let table = channels
        .entry(channel.to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("{} has non-table `channels.{channel}`", path.display()))?;
    for (key, value) in values {
        table.insert(key.to_string(), value);
    }
    Ok((path, toml::to_string_pretty(&root)?))
}

fn env_assignment(key: &str, value: &str) -> String {
    let plain = value.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/' | '@' | '+' | '=')
    });
    let value = if plain {
        value.to_string()
    } else {
        serde_json::to_string(value).expect("string serialization cannot fail")
    };
    format!("{key}={value}")
}

/// Replace a file only after the complete new body has reached a sibling
/// temporary file. Existing mode bits are retained; secret env files force
/// owner-only permissions.
fn atomic_write(path: &Path, body: &str, mode: Option<u32>) -> anyhow::Result<()> {
    let tmp = path.with_file_name(format!(
        ".{}.tmp.{}.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("komo"),
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode.unwrap_or(0o600));
    }
    let mut file = options.open(&tmp)?;
    let result = (|| -> anyhow::Result<()> {
        file.write_all(body.as_bytes())?;
        file.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions =
                mode.or_else(|| std::fs::metadata(path).ok().map(|m| m.permissions().mode()));
            if let Some(permissions) = permissions {
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(permissions))?;
            }
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_update_activates_a_commented_credential_without_losing_other_lines() {
        let home =
            std::env::temp_dir().join(format!("komo_write_env_test_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(".env"), "# TELEGRAM_BOT_TOKEN=\nOTHER=value\n").unwrap();

        write_env_values(&home, &[("TELEGRAM_BOT_TOKEN", "token")]).unwrap();

        assert_eq!(
            std::fs::read_to_string(home.join(".env")).unwrap(),
            "TELEGRAM_BOT_TOKEN=token\nOTHER=value\n"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn env_update_replaces_every_duplicate_and_quotes_special_values() {
        let home = std::env::temp_dir().join(format!(
            "komo_write_duplicate_env_test_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join(".env"),
            "TELEGRAM_BOT_TOKEN=old\n# TELEGRAM_BOT_TOKEN=template\nTELEGRAM_BOT_TOKEN=older\n",
        )
        .unwrap();

        write_env_values(&home, &[("TELEGRAM_BOT_TOKEN", "has # and spaces")]).unwrap();

        assert_eq!(
            std::fs::read_to_string(home.join(".env")).unwrap(),
            "TELEGRAM_BOT_TOKEN=\"has # and spaces\"\n"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn channel_config_preserves_unrelated_settings() {
        let home =
            std::env::temp_dir().join(format!("komo_write_channel_test_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("config.toml"), "provider = \"codex\"\n").unwrap();

        write_channel_config(&home, "telegram", [("enabled", toml::Value::Boolean(true))]).unwrap();

        let value: toml::Value =
            toml::from_str(&std::fs::read_to_string(home.join("config.toml")).unwrap()).unwrap();
        assert_eq!(value["provider"].as_str(), Some("codex"));
        assert_eq!(
            value["channels"]["telegram"]["enabled"].as_bool(),
            Some(true)
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
