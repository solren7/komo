//! Model inspection and switching (`shion model list`, `shion model set`).
//!
//! `list` shows the resolved provider/model and where each value comes from
//! (env var > config.toml > built-in default), plus every available provider.
//! `set` persists a new selection into `~/.shion/config.toml`. Neither touches
//! the database or requires the API key to be present.

use crate::{
    config::{self, FileConfig, Provider, Secrets, ShionEnv},
    infra::codex::{self, CodexAuth},
};

fn key_present(keys: &Secrets, provider: Provider) -> bool {
    keys.key(provider).is_some()
}

fn auth_present(provider: Provider, keys: &Secrets) -> bool {
    match provider {
        Provider::Codex => CodexAuth::load().is_ok(),
        _ => key_present(keys, provider),
    }
}

fn credential_line(provider: Provider, keys: &Secrets) -> String {
    match provider {
        Provider::Codex => format!(
            "Codex OAuth {}  {}",
            codex::codex_auth_file_path().display(),
            if auth_present(provider, keys) {
                "✓ logged in"
            } else {
                "✗ missing"
            }
        ),
        _ => format!(
            "{}  {}",
            provider.api_key_var(),
            if key_present(keys, provider) {
                "✓ set"
            } else {
                "✗ missing"
            }
        ),
    }
}

fn resolve_set_args(
    provider_or_model: &str,
    model: Option<String>,
) -> anyhow::Result<(Provider, Option<String>, bool)> {
    match Provider::parse(provider_or_model) {
        Ok(provider) => Ok((provider, model, false)),
        Err(parse_err) => {
            if model.is_none() && codex::looks_like_codex_model_id(provider_or_model) {
                Ok((Provider::Codex, Some(provider_or_model.to_string()), true))
            } else {
                Err(parse_err)
            }
        }
    }
}

async fn preferred_codex_model() -> String {
    let token = match CodexAuth::load() {
        Ok(auth) => auth.resolve().await.ok(),
        Err(_) => None,
    };
    codex::codex_model_ids(token.as_deref())
        .await
        .into_iter()
        .next()
        .unwrap_or_else(|| Provider::Codex.default_model().to_string())
}

/// Show the current provider/model (with its source) and list all providers.
pub async fn list() -> anyhow::Result<()> {
    let home = config::ensure_shion_home();
    let file = FileConfig::load(&home);
    let env = ShionEnv::load_lenient();
    let keys = Secrets::load();

    let provider_str = env
        .provider
        .clone()
        .or_else(|| file.provider.clone())
        .unwrap_or_else(|| "deepseek".to_string());
    let provider = Provider::parse(&provider_str)?;
    let provider_source = if env.provider.is_some() {
        "env SHION_PROVIDER"
    } else if file.provider.is_some() {
        "config.toml"
    } else {
        "default"
    };

    let model = env
        .model
        .clone()
        .or_else(|| file.model.clone())
        .unwrap_or_else(|| provider.default_model().to_string());
    let model_source = if env.model.is_some() {
        "env SHION_MODEL"
    } else if file.model.is_some() {
        "config.toml"
    } else {
        "provider default"
    };

    println!("Current");
    println!("  provider  {}  ({provider_source})", provider.name());
    println!("  model     {model}  ({model_source})");
    println!("  auth      {}", credential_line(provider, &keys));

    if provider == Provider::Codex {
        let token = match CodexAuth::load() {
            Ok(auth) => auth.resolve().await.ok(),
            Err(_) => None,
        };
        let models = codex::codex_model_ids(token.as_deref()).await;
        if !models.is_empty() {
            println!(
                "  codex models {}",
                models
                    .iter()
                    .take(6)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    println!();
    println!("Available providers  (* = active)");
    for p in Provider::ALL {
        println!(
            "  {} {:<11} default {:<26} auth {}",
            if p == provider { "*" } else { " " },
            p.name(),
            p.default_model(),
            if auth_present(p, &keys) { "✓" } else { "·" },
        );
    }

    println!();
    println!("Switch with: shion model set <provider> [model]");
    println!("Codex shortcut: shion model set gpt-5.5");
    Ok(())
}

/// Switch the provider (and optionally the model), persisting to config.toml.
pub async fn set(provider_str: &str, model: Option<String>) -> anyhow::Result<()> {
    let home = config::ensure_shion_home();
    let (provider, model, inferred_provider) = resolve_set_args(provider_str, model)?;
    let resolved_model = match (provider, model) {
        (Provider::Codex, None) => Some(preferred_codex_model().await),
        (_, model) => model,
    };
    let path = config::write_model_selection(&home, provider, resolved_model.as_deref())?;

    let effective = resolved_model
        .clone()
        .unwrap_or_else(|| provider.default_model().to_string());
    println!("provider = {}", provider.name());
    if inferred_provider {
        println!("model    = {effective}  (inferred codex provider)");
    } else if resolved_model.is_some() {
        println!("model    = {effective}");
    } else {
        println!("model    = {effective}  (provider default)");
    }
    println!("wrote {}", path.display());

    let env = ShionEnv::load_lenient();
    if env.provider.is_some() || env.model.is_some() {
        eprintln!(
            "note: SHION_PROVIDER/SHION_MODEL are set and override config.toml; \
             unset them for this change to take effect"
        );
    }
    let keys = Secrets::load();
    if !auth_present(provider, &keys) {
        match provider {
            Provider::Codex => eprintln!(
                "note: Codex OAuth credentials are missing at {}; run `codex` to log in before using codex",
                codex::codex_auth_file_path().display()
            ),
            _ => eprintln!(
                "note: {} is not set — add it to {}/.env before using {}",
                provider.api_key_var(),
                home.display(),
                provider.name()
            ),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_args_accept_provider_plus_model() {
        let (provider, model, inferred) =
            resolve_set_args("openai", Some("gpt-4o".into())).unwrap();
        assert_eq!(provider, Provider::OpenAi);
        assert_eq!(model.as_deref(), Some("gpt-4o"));
        assert!(!inferred);
    }

    #[test]
    fn set_args_infers_codex_provider_from_codex_model() {
        let (provider, model, inferred) = resolve_set_args("gpt-5.5", None).unwrap();
        assert_eq!(provider, Provider::Codex);
        assert_eq!(model.as_deref(), Some("gpt-5.5"));
        assert!(inferred);
    }

    #[test]
    fn set_args_keeps_non_codex_models_as_unknown_providers() {
        assert!(resolve_set_args("gpt-4o-mini", None).is_err());
    }
}
