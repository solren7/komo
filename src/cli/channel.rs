//! Channel inventory, setup, and read-only connectivity checks.
//!
//! The public interface is intentionally small: `list` renders the resolved
//! configuration plus the gateway's mounted-channel snapshot; `probe` checks
//! one provider without sending a message; `setup` owns the interactive
//! credential/configuration write path.

use std::{
    io::{self, IsTerminal, Write},
    time::Duration,
};

use serde::Serialize;
use serde_json::json;

use crate::{
    config::{ApiConfig, ChannelState, ConfigSnapshot},
    infra::gateway_client::GatewayClient,
};

#[cfg(unix)]
struct EchoGuard {
    fd: std::os::fd::RawFd,
    original: libc::termios,
}

#[cfg(unix)]
impl Drop for EchoGuard {
    fn drop(&mut self) {
        // Best effort only: a read error must not leave the operator's shell
        // without echo. (SIGKILL cannot be recovered by any terminal helper.)
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelSummary {
    pub name: &'static str,
    pub kind: &'static str,
    pub config: String,
    pub gateway: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// List every built-in channel. This remains useful with no gateway running:
/// configuration is always rendered, while the mounted-channel column becomes
/// `unavailable` rather than failing the command.
pub async fn list(config: &ConfigSnapshot, json: bool) -> anyhow::Result<()> {
    let gateway_channels = match GatewayClient::try_connect().await {
        Some(client) => client.status().await.ok().map(|status| status.channels),
        None => None,
    };
    let rows = summaries(config, gateway_channels.as_deref());
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    println!("CHANNEL         KIND      CONFIG             GATEWAY");
    for row in rows {
        println!(
            "{:<15} {:<9} {:<18} {}",
            row.name, row.kind, row.config, row.gateway
        );
        if let Some(detail) = row.detail {
            println!("  {detail}");
        }
    }
    Ok(())
}

pub async fn probe(_config: &ConfigSnapshot, name: &str) -> anyhow::Result<()> {
    let name = normalize_name(name)?;
    match name.as_str() {
        "feishu" => probe_feishu(_config).await,
        "telegram" => probe_telegram(_config).await,
        "wechat" => probe_wechat(_config),
        "homeassistant" => probe_homeassistant(_config).await,
        "api" => probe_api().await,
        _ => unreachable!("normalize_name accepts only built-in channels"),
    }
}

pub async fn setup(config: &ConfigSnapshot, name: &str) -> anyhow::Result<()> {
    let name = normalize_name(name)?;
    if name == "api" {
        anyhow::bail!(
            "the api channel is already available on loopback. Configure [channels.api] manually \
             only when you intentionally need external exposure"
        );
    }
    require_terminal()?;
    match name.as_str() {
        "feishu" => setup_feishu(config),
        "telegram" => setup_telegram(config),
        "wechat" => setup_wechat(config).await,
        "homeassistant" => setup_homeassistant(config),
        "api" => unreachable!("api is handled before the interactive-terminal check"),
        _ => unreachable!("normalize_name accepts only built-in channels"),
    }
}

fn normalize_name(name: &str) -> anyhow::Result<String> {
    let value = name.trim().to_ascii_lowercase();
    match value.as_str() {
        "feishu" | "telegram" | "wechat" | "homeassistant" | "api" => Ok(value),
        _ => anyhow::bail!(
            "unknown channel `{name}` (expected feishu | telegram | wechat | homeassistant | api)"
        ),
    }
}

fn require_terminal() -> anyhow::Result<()> {
    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        return Ok(());
    }
    anyhow::bail!("`komo channel setup` needs an interactive terminal")
}

fn prompt(label: &str, required: bool) -> anyhow::Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let value = value.trim().to_string();
    if required && value.is_empty() {
        anyhow::bail!("{label} is required")
    }
    Ok(value)
}

/// Read a secret without leaving it in the terminal scrollback. The unix
/// implementation is deliberately local: operator setup runs on the host
/// terminal, while non-unix builds retain a functional (but visible) fallback.
fn prompt_secret(label: &str) -> anyhow::Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;

        let stdin = io::stdin();
        let fd = stdin.as_raw_fd();
        let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(fd, original.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        let original = unsafe { original.assume_init() };
        let mut hidden = original;
        hidden.c_lflag &= !libc::ECHO;
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &hidden) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        let restore = EchoGuard { fd, original };
        let mut value = String::new();
        let read = stdin.read_line(&mut value);
        drop(restore);
        println!();
        read?;
        let value = value.trim().to_string();
        if value.is_empty() {
            anyhow::bail!("{label} is required");
        }
        return Ok(value);
    }
    #[cfg(not(unix))]
    {
        eprintln!("warning: secret input is visible on this platform");
        prompt(label, true)
    }
}

fn confirm(label: &str) -> anyhow::Result<bool> {
    let answer = prompt(label, false)?;
    Ok(matches!(answer.to_ascii_lowercase().as_str(), "y" | "yes"))
}

fn csv(value: String) -> toml::Value {
    toml::Value::Array(
        value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| toml::Value::String(item.to_string()))
            .collect(),
    )
}

fn report_setup(config_path: &std::path::Path, env_path: Option<&std::path::Path>) {
    println!("configured {}", config_path.display());
    if let Some(env_path) = env_path {
        println!("saved credentials to {}", env_path.display());
    }
    println!("Restart the gateway to apply this channel: `komo gateway restart`.");
}

fn setup_feishu(config: &ConfigSnapshot) -> anyhow::Result<()> {
    println!("Feishu setup — create an app, then enter its App ID and App Secret.");
    let app_id = prompt("FEISHU_APP_ID: ", true)?;
    let app_secret = prompt_secret("FEISHU_APP_SECRET: ")?;
    crate::config::validate_channel_config(
        &config.runtime.home,
        "feishu",
        [("enabled", toml::Value::Boolean(true))],
    )?;
    let env_path = crate::config::write_env_values(
        &config.runtime.home,
        &[
            ("FEISHU_APP_ID", &app_id),
            ("FEISHU_APP_SECRET", &app_secret),
        ],
    )?;
    let config_path = crate::config::write_channel_config(
        &config.runtime.home,
        "feishu",
        [("enabled", toml::Value::Boolean(true))],
    )?;
    report_setup(&config_path, Some(&env_path));
    Ok(())
}

fn setup_telegram(config: &ConfigSnapshot) -> anyhow::Result<()> {
    println!("Telegram setup — create a bot with BotFather, then enter its token.");
    let token = prompt_secret("TELEGRAM_BOT_TOKEN: ")?;
    crate::config::validate_channel_config(
        &config.runtime.home,
        "telegram",
        [("enabled", toml::Value::Boolean(true))],
    )?;
    let env_path =
        crate::config::write_env_values(&config.runtime.home, &[("TELEGRAM_BOT_TOKEN", &token)])?;
    let config_path = crate::config::write_channel_config(
        &config.runtime.home,
        "telegram",
        [("enabled", toml::Value::Boolean(true))],
    )?;
    report_setup(&config_path, Some(&env_path));
    Ok(())
}

async fn setup_wechat(config: &ConfigSnapshot) -> anyhow::Result<()> {
    println!("WeChat setup — scan the QR code and confirm on your phone.");
    crate::config::validate_channel_config(
        &config.runtime.home,
        "wechat",
        [("enabled", toml::Value::Boolean(true))],
    )?;
    crate::cli::wechat::login().await?;
    let config_path = crate::config::write_channel_config(
        &config.runtime.home,
        "wechat",
        [("enabled", toml::Value::Boolean(true))],
    )?;
    report_setup(&config_path, None);
    Ok(())
}

fn setup_homeassistant(config: &ConfigSnapshot) -> anyhow::Result<()> {
    println!("Home Assistant setup — enter a long-lived access token.");
    let token = prompt_secret("HASS_TOKEN: ")?;
    let url = prompt(
        "HASS_URL (blank = http://homeassistant.local:8123): ",
        false,
    )?;
    let watch_all = confirm("Forward every state change? [y/N]: ")?;
    let domains = if watch_all {
        String::new()
    } else {
        prompt("Watch domains (comma-separated; blank = none): ", false)?
    };
    let entities = if watch_all {
        String::new()
    } else {
        prompt("Watch entities (comma-separated; blank = none): ", false)?
    };
    if !watch_all && domains.trim().is_empty() && entities.trim().is_empty() {
        eprintln!("note: no watches selected, so Home Assistant will not forward events yet");
    }
    let base_url = if url.is_empty() {
        "http://homeassistant.local:8123"
    } else {
        url.as_str()
    };
    let env_values = [("HASS_TOKEN", token.as_str()), ("HASS_URL", base_url)];
    let channel_values = [
        ("enabled", toml::Value::Boolean(true)),
        ("watch_all", toml::Value::Boolean(watch_all)),
        ("watch_domains", csv(domains)),
        ("watch_entities", csv(entities)),
    ];
    crate::config::validate_channel_config(
        &config.runtime.home,
        "homeassistant",
        channel_values.clone(),
    )?;
    let env_path = crate::config::write_env_values(&config.runtime.home, &env_values)?;
    let config_path =
        crate::config::write_channel_config(&config.runtime.home, "homeassistant", channel_values)?;
    report_setup(&config_path, Some(&env_path));
    Ok(())
}

fn http_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?)
}

fn require_ready<'a, T>(name: &str, state: &'a ChannelState<T>) -> anyhow::Result<&'a T> {
    match state {
        ChannelState::Ready(config) => Ok(config),
        ChannelState::Disabled => {
            anyhow::bail!("{name} is disabled; run `komo channel setup {name}` first")
        }
        ChannelState::Misconfigured(error) => anyhow::bail!("{name} is misconfigured: {error}"),
    }
}

async fn probe_feishu(config: &ConfigSnapshot) -> anyhow::Result<()> {
    let channel = require_ready("feishu", &config.runtime.feishu)?;
    let response = http_client()?
        .post("https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal")
        .json(&json!({ "app_id": channel.app_id, "app_secret": channel.app_secret }))
        .send()
        .await
        .map_err(|_| {
            anyhow::anyhow!("Feishu authentication request failed; check network connectivity")
        })?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or_default();
    if !status.is_success() || body.get("code").and_then(|value| value.as_i64()) != Some(0) {
        anyhow::bail!(
            "Feishu authentication failed ({status}): {}",
            body.get("msg")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown error")
        );
    }
    println!("✓ feishu credentials accepted");
    Ok(())
}

async fn probe_telegram(config: &ConfigSnapshot) -> anyhow::Result<()> {
    let channel = require_ready("telegram", &config.runtime.telegram)?;
    let response = http_client()?
        .get(format!(
            "https://api.telegram.org/bot{}/getMe",
            channel.bot_token
        ))
        .send()
        .await
        .map_err(|_| {
            anyhow::anyhow!("Telegram authentication request failed; check network connectivity")
        })?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or_default();
    if !status.is_success() || body.get("ok").and_then(|value| value.as_bool()) != Some(true) {
        anyhow::bail!(
            "Telegram authentication failed ({status}): {}",
            body.get("description")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown error")
        );
    }
    let identity = body["result"]["username"].as_str().unwrap_or("bot");
    println!("✓ telegram credentials accepted (@{identity})");
    Ok(())
}

fn probe_wechat(config: &ConfigSnapshot) -> anyhow::Result<()> {
    require_ready("wechat", &config.runtime.wechat)?;
    let path = crate::config::wechat_cred_path();
    if !path.exists() {
        anyhow::bail!("WeChat is enabled but not logged in; run `komo channel wechat login`");
    }
    let body = std::fs::read_to_string(&path)?;
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|_| {
        anyhow::anyhow!("WeChat credential file is not valid JSON; run `komo channel wechat login`")
    })?;
    if !value.is_object() {
        anyhow::bail!(
            "WeChat credential file has an unexpected format; run `komo channel wechat login`"
        );
    }
    anyhow::bail!(
        "WeChat credential file is valid ({}) but live iLink probing is unsupported without opening a bot session; no message was sent",
        path.display()
    );
}

async fn probe_homeassistant(config: &ConfigSnapshot) -> anyhow::Result<()> {
    let channel = require_ready("homeassistant", &config.runtime.homeassistant_channel)?;
    let response = http_client()?
        .get(format!("{}/api/", channel.base_url.trim_end_matches('/')))
        .bearer_auth(&channel.token)
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!(
            "Home Assistant authentication failed ({})",
            response.status()
        );
    }
    println!("✓ homeassistant credentials accepted");
    Ok(())
}

async fn probe_api() -> anyhow::Result<()> {
    let client = GatewayClient::try_connect()
        .await
        .ok_or_else(|| anyhow::anyhow!("gateway is not reachable"))?;
    let status = client.status().await?;
    println!(
        "✓ api gateway reachable (channels: {})",
        status.channels.join(", ")
    );
    Ok(())
}

pub fn summaries(
    config: &ConfigSnapshot,
    gateway_channels: Option<&[String]>,
) -> Vec<ChannelSummary> {
    let rt = &config.runtime;
    vec![
        summary("feishu", "chat", state_status(&rt.feishu), gateway_channels),
        summary(
            "telegram",
            "chat",
            state_status(&rt.telegram),
            gateway_channels,
        ),
        summary(
            "wechat",
            "chat",
            wechat_status(&rt.wechat),
            gateway_channels,
        ),
        summary(
            "homeassistant",
            "event",
            state_status(&rt.homeassistant_channel),
            gateway_channels,
        ),
        summary("api", "control", api_status(&rt.api), gateway_channels),
    ]
}

fn summary(
    name: &'static str,
    kind: &'static str,
    (config, detail): (String, Option<String>),
    gateway_channels: Option<&[String]>,
) -> ChannelSummary {
    let gateway = match gateway_channels {
        Some(channels) if channels.iter().any(|channel| channel == name) => "loaded".to_string(),
        Some(_) if config != "disabled" => "not loaded (restart needed)".to_string(),
        Some(_) => "not loaded".to_string(),
        None => "unavailable".to_string(),
    };
    ChannelSummary {
        name,
        kind,
        config,
        gateway,
        detail,
    }
}

fn state_status<T>(state: &ChannelState<T>) -> (String, Option<String>) {
    match state {
        ChannelState::Disabled => ("disabled".to_string(), None),
        ChannelState::Ready(_) => ("ready".to_string(), None),
        ChannelState::Misconfigured(error) => ("misconfigured".to_string(), Some(error.clone())),
    }
}

fn wechat_status(state: &ChannelState<crate::config::WeChatConfig>) -> (String, Option<String>) {
    match state {
        ChannelState::Ready(_) if !crate::config::wechat_cred_path().exists() => (
            "login required".to_string(),
            Some("run `komo channel wechat login` or `komo channel setup wechat`".to_string()),
        ),
        _ => state_status(state),
    }
}

fn api_status(state: &ChannelState<ApiConfig>) -> (String, Option<String>) {
    match state {
        ChannelState::Ready(config) if config.port == 0 => (
            "loopback".to_string(),
            Some("local CLI control channel".to_string()),
        ),
        ChannelState::Ready(config) => (
            "external".to_string(),
            Some(format!("{}:{}", config.bind, config.port)),
        ),
        _ => state_status(state),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_mark_missing_wechat_login_and_stale_gateway() {
        let wechat = summary(
            "wechat",
            "chat",
            ("login required".to_string(), None),
            Some(&["telegram".to_string()]),
        );
        assert_eq!(wechat.config, "login required");

        let telegram = summary(
            "telegram",
            "chat",
            ("ready".to_string(), None),
            Some(&["feishu".to_string()]),
        );
        assert_eq!(telegram.gateway, "not loaded (restart needed)");
    }
}
