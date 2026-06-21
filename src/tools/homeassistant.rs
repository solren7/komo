use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::domain::{
    approval::{ApprovalRequest, Approver},
    tool::Tool,
};

/// Cap on the textual size of a tool result (entity/service lists are large).
const MAX_BYTES: usize = 8 * 1024;

/// Per-request HTTP timeout — a hung HA instance must not wedge a turn.
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// Service domains refused outright (hermes' "blocked domains" floor). Home
/// Assistant has *no* service-level access control: anyone with the token can
/// call these to run arbitrary commands as root in the HA container or make the
/// HA server issue arbitrary HTTP requests (SSRF on the LAN). No approval
/// unlocks them — the floor lives in this client, like shell's hardline list.
const BLOCKED_DOMAINS: &[&str] = &[
    "shell_command", // arbitrary shell commands in the HA container
    "command_line",  // sensors/switches that execute shell commands
    "python_script", // sandboxed but can escalate via hass.services.call()
    "pyscript",      // scripting integration with broader access
    "hassio",        // addon control, host shutdown/reboot
    "rest_command",  // arbitrary HTTP requests from the HA server (SSRF)
];

#[derive(Deserialize)]
struct HassArgs {
    action: String,
    /// Service domain (e.g. "light", "switch") for call_service / list_services,
    /// or an entity-id prefix filter for list_entities.
    #[serde(default)]
    domain: Option<String>,
    /// Service name (e.g. "turn_on") for call_service.
    #[serde(default)]
    service: Option<String>,
    /// Target entity id (e.g. "light.kitchen") for get_state / call_service.
    #[serde(default)]
    entity_id: Option<String>,
    /// Area/room name filter for list_entities (matched against friendly_name).
    #[serde(default)]
    area: Option<String>,
    /// Extra service data merged into the call_service body (e.g.
    /// {"brightness_pct": 50}).
    #[serde(default)]
    data: Option<Value>,
}

/// Talks to a Home Assistant instance over its REST API: read entity states,
/// discover services, and call services (turn devices on/off, etc.). Configured
/// via `HASS_TOKEN` + `HASS_URL` (see `config.rs`).
pub struct HomeAssistantTool {
    client: reqwest::Client,
    base_url: String,
    token: String,
    approver: Arc<dyn Approver>,
}

impl HomeAssistantTool {
    pub fn new(base_url: String, token: String, approver: Arc<dyn Approver>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(HTTP_TIMEOUT)
                .build()
                .expect("failed to build reqwest client"),
            base_url,
            token,
            approver,
        }
    }

    async fn get_json(&self, path: &str) -> anyhow::Result<Value> {
        let resp = self
            .client
            .get(format!("{}{path}", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("request to Home Assistant failed: {e}"))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("Home Assistant returned HTTP {status}: {body}");
        }
        serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("invalid JSON from Home Assistant: {e}"))
    }
}

#[async_trait]
impl Tool for HomeAssistantTool {
    fn name(&self) -> &'static str {
        "homeassistant"
    }

    fn description(&self) -> &'static str {
        "Query and control a Home Assistant smart-home instance. \
         action=\"list_entities\" lists entities + current state (optional \
         `domain` and/or `area` filter); \
         action=\"get_state\" returns one entity's full state + attributes \
         (requires `entity_id`); \
         action=\"list_services\" discovers callable services per domain (use it \
         to learn what `call_service` accepts); \
         action=\"call_service\" invokes a service to change something (requires \
         `domain` + `service`, e.g. light/turn_on, usually with `entity_id`, \
         plus optional `data`). Control actions ask for approval."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list_entities", "get_state", "list_services", "call_service"],
                    "description": "The operation to perform."
                },
                "domain": {
                    "type": "string",
                    "description": "Service domain for call_service/list_services (e.g. \"light\"); or an entity-id prefix filter for list_entities."
                },
                "service": {
                    "type": "string",
                    "description": "Service name for call_service (e.g. \"turn_on\", \"turn_off\", \"toggle\", \"set_temperature\")."
                },
                "entity_id": {
                    "type": "string",
                    "description": "Target entity id (e.g. \"light.kitchen\") for get_state or call_service."
                },
                "area": {
                    "type": "string",
                    "description": "Area/room name filter for list_entities (e.g. \"kitchen\"); matched against friendly names."
                },
                "data": {
                    "type": "object",
                    "description": "Extra service data for call_service, e.g. {\"brightness_pct\": 50}."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: String) -> anyhow::Result<String> {
        let args: HassArgs = serde_json::from_str(&input)
            .map_err(|e| anyhow::anyhow!("invalid homeassistant arguments: {e}"))?;

        match args.action.as_str() {
            "list_entities" => {
                let states = self.get_json("/api/states").await?;
                let mut out =
                    format_entities(&states, args.domain.as_deref(), args.area.as_deref());
                truncate_to_char_boundary(&mut out, MAX_BYTES);
                Ok(out)
            }

            "list_services" => {
                // Validate the optional domain filter so a bogus value can't be
                // mistaken for "no filter".
                if let Some(d) = &args.domain
                    && !valid_name(d)
                {
                    return Ok(format!("Refused: invalid domain `{d}`."));
                }
                let services = self.get_json("/api/services").await?;
                let mut out = format_services(&services, args.domain.as_deref());
                truncate_to_char_boundary(&mut out, MAX_BYTES);
                Ok(out)
            }

            "get_state" => {
                let id = args.entity_id.ok_or_else(|| {
                    anyhow::anyhow!("`entity_id` is required for action=get_state")
                })?;
                if !valid_entity_id(&id) {
                    return Ok(format!("Refused: invalid entity_id `{id}`."));
                }
                let state = self.get_json(&format!("/api/states/{id}")).await?;
                let mut out =
                    serde_json::to_string_pretty(&state).unwrap_or_else(|_| state.to_string());
                truncate_to_char_boundary(&mut out, MAX_BYTES);
                Ok(out)
            }

            "call_service" => {
                let domain = args.domain.ok_or_else(|| {
                    anyhow::anyhow!("`domain` is required for action=call_service")
                })?;
                let service = args.service.ok_or_else(|| {
                    anyhow::anyhow!("`service` is required for action=call_service")
                })?;

                // Validate name *shape* before anything else: the domain and
                // service are interpolated into the request path, so rejecting
                // non-`[a-z_0-9]` tokens closes path-traversal/SSRF (e.g.
                // domain="../../api/config") and blocklist-bypass (e.g.
                // "shell_command/../light").
                if !valid_name(&domain) {
                    return Ok(format!("Refused: invalid service domain `{domain}`."));
                }
                if !valid_name(&service) {
                    return Ok(format!("Refused: invalid service name `{service}`."));
                }
                if BLOCKED_DOMAINS.contains(&domain.as_str()) {
                    return Ok(format!(
                        "Refused: service domain `{domain}` is blocked for security \
                         (arbitrary code execution / SSRF on the HA host). Blocked: {}.",
                        BLOCKED_DOMAINS.join(", ")
                    ));
                }
                if let Some(id) = &args.entity_id
                    && !valid_entity_id(id)
                {
                    return Ok(format!("Refused: invalid entity_id `{id}`."));
                }

                // Assemble the service body: caller-supplied `data` (must be an
                // object) plus the target `entity_id` if given.
                let mut body = match args.data {
                    Some(Value::Object(m)) => m,
                    Some(_) => anyhow::bail!("`data` must be a JSON object"),
                    None => serde_json::Map::new(),
                };
                if let Some(id) = &args.entity_id {
                    body.insert("entity_id".to_string(), json!(id));
                }

                // Changing physical-world state is side-effecting: gate it
                // through the approver (session-scoped per service so repeats of
                // the same action don't re-prompt). This sits *above* the
                // blocklist floor — blocked domains never reach here.
                let target = args
                    .entity_id
                    .as_deref()
                    .map(|id| format!(" on {id}"))
                    .unwrap_or_default();
                let request =
                    ApprovalRequest::normal(format!("Home Assistant: {domain}.{service}{target}"))
                        .with_scope_key(format!("homeassistant:{domain}.{service}"));
                if !self.approver.approve(&request).await {
                    return Ok("Service call rejected by user; nothing was changed.".to_string());
                }

                let resp = self
                    .client
                    .post(format!("{}/api/services/{domain}/{service}", self.base_url))
                    .bearer_auth(&self.token)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| anyhow::anyhow!("request to Home Assistant failed: {e}"))?;
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                if !status.is_success() {
                    anyhow::bail!("Home Assistant returned HTTP {status}: {text}");
                }
                // The response is the array of entities that changed state.
                let changed = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v.as_array().map(|a| a.len()))
                    .unwrap_or(0);
                Ok(format!(
                    "Called {domain}.{service}{target}; {changed} entit{} changed.",
                    if changed == 1 { "y" } else { "ies" }
                ))
            }

            other => Err(anyhow::anyhow!(
                "unknown action `{other}` (expected list_entities/get_state/list_services/call_service)"
            )),
        }
    }
}

/// A valid HA domain/service token: `^[a-z][a-z0-9_]*$`. Rejecting anything
/// else stops path traversal / SSRF when the token is interpolated into
/// `/api/services/{domain}/{service}`.
fn valid_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A valid HA entity id: `^[a-z_][a-z0-9_]*\.[a-z0-9_]+$` — exactly one dot,
/// with a valid domain and a non-empty object id.
fn valid_entity_id(s: &str) -> bool {
    let Some((domain, object)) = s.split_once('.') else {
        return false;
    };
    if object.is_empty() || object.contains('.') {
        return false;
    }
    let domain_ok = {
        let mut c = domain.chars();
        matches!(c.next(), Some(ch) if ch.is_ascii_lowercase() || ch == '_')
            && c.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    };
    let object_ok = object
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    domain_ok && object_ok
}

/// Render `/api/states` (a JSON array) into sorted `entity_id = state (name)`
/// lines, optionally keeping only entities in `domain` (the `domain.` entity-id
/// prefix) and/or whose friendly name / area mentions `area`.
fn format_entities(states: &Value, domain: Option<&str>, area: Option<&str>) -> String {
    let Some(arr) = states.as_array() else {
        return "Unexpected response from Home Assistant.".to_string();
    };
    let area_lc = area.map(str::to_lowercase);
    let mut lines: Vec<String> = arr
        .iter()
        .filter_map(|s| {
            let id = s.get("entity_id").and_then(Value::as_str)?;
            if let Some(d) = domain
                && !id.starts_with(&format!("{d}."))
            {
                return None;
            }
            let attrs = s.get("attributes");
            let name = attrs
                .and_then(|a| a.get("friendly_name"))
                .and_then(Value::as_str);
            if let Some(area_lc) = &area_lc {
                let hay_name = name.unwrap_or("").to_lowercase();
                let hay_area = attrs
                    .and_then(|a| a.get("area"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_lowercase();
                if !hay_name.contains(area_lc.as_str()) && !hay_area.contains(area_lc.as_str()) {
                    return None;
                }
            }
            let state = s.get("state").and_then(Value::as_str).unwrap_or("unknown");
            Some(match name {
                Some(n) => format!("{id} = {state} ({n})"),
                None => format!("{id} = {state}"),
            })
        })
        .collect();
    lines.sort();
    if lines.is_empty() {
        "No matching entities found.".to_string()
    } else {
        lines.join("\n")
    }
}

/// Render `/api/services` into compact `domain.service — description` lines,
/// optionally limited to one `domain`.
fn format_services(services: &Value, domain: Option<&str>) -> String {
    let Some(arr) = services.as_array() else {
        return "Unexpected response from Home Assistant.".to_string();
    };
    let mut lines = Vec::new();
    for entry in arr {
        let d = entry.get("domain").and_then(Value::as_str).unwrap_or("");
        if let Some(want) = domain
            && d != want
        {
            continue;
        }
        if let Some(map) = entry.get("services").and_then(Value::as_object) {
            for (name, info) in map {
                let desc = info
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if desc.is_empty() {
                    lines.push(format!("{d}.{name}"));
                } else {
                    lines.push(format!("{d}.{name} — {desc}"));
                }
            }
        }
    }
    if lines.is_empty() {
        "No matching services found.".to_string()
    } else {
        lines.join("\n")
    }
}

/// Truncates to at most `max_bytes`, backing up so the cut never splits a
/// multi-byte UTF-8 character (mirrors `web_fetch`).
fn truncate_to_char_boundary(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str("\n…[truncated]");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::approval::{ApprovalRequest, Approver};

    fn states() -> Value {
        json!([
            {"entity_id": "light.kitchen", "state": "on",
             "attributes": {"friendly_name": "Kitchen Light"}},
            {"entity_id": "light.bedroom", "state": "off",
             "attributes": {"friendly_name": "Bedroom Light"}},
            {"entity_id": "switch.fan", "state": "off", "attributes": {}}
        ])
    }

    // ── validation ────────────────────────────────────────────────────────

    #[test]
    fn valid_name_accepts_ha_tokens_and_rejects_traversal() {
        assert!(valid_name("light"));
        assert!(valid_name("turn_on"));
        assert!(valid_name("media_player"));
        assert!(!valid_name("")); // empty
        assert!(!valid_name("Light")); // uppercase
        assert!(!valid_name("3d")); // leading digit
        assert!(!valid_name("../../api/config")); // path traversal
        assert!(!valid_name("shell_command/../light")); // bypass attempt
    }

    #[test]
    fn valid_entity_id_requires_one_dot_and_clean_parts() {
        assert!(valid_entity_id("light.kitchen"));
        assert!(valid_entity_id("sensor.temperature_1"));
        assert!(!valid_entity_id("light")); // no dot
        assert!(!valid_entity_id("light.")); // empty object
        assert!(!valid_entity_id("light.a.b")); // two dots
        assert!(!valid_entity_id("../secrets")); // traversal
        assert!(!valid_entity_id("Light.Kitchen")); // uppercase
    }

    // ── format_entities ───────────────────────────────────────────────────

    #[test]
    fn format_entities_renders_sorted_lines_with_names() {
        let out = format_entities(&states(), None, None);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "light.bedroom = off (Bedroom Light)");
        assert_eq!(lines[1], "light.kitchen = on (Kitchen Light)");
        assert_eq!(lines[2], "switch.fan = off");
    }

    #[test]
    fn format_entities_filters_by_domain() {
        let out = format_entities(&states(), Some("light"), None);
        assert_eq!(out.lines().count(), 2);
        assert!(out.contains("light.kitchen"));
        assert!(!out.contains("switch.fan"));
    }

    #[test]
    fn format_entities_filters_by_area_against_friendly_name() {
        let out = format_entities(&states(), None, Some("kitchen"));
        assert_eq!(out, "light.kitchen = on (Kitchen Light)");
    }

    #[test]
    fn format_entities_empty_filter_reports_none() {
        let out = format_entities(&states(), Some("climate"), None);
        assert_eq!(out, "No matching entities found.");
    }

    // ── format_services ───────────────────────────────────────────────────

    #[test]
    fn format_services_compacts_and_filters_by_domain() {
        let services = json!([
            {"domain": "light", "services": {
                "turn_on": {"description": "Turn on lights"},
                "turn_off": {"description": "Turn off lights"}}},
            {"domain": "switch", "services": {
                "toggle": {"description": "Toggle a switch"}}}
        ]);
        let out = format_services(&services, Some("light"));
        assert!(out.contains("light.turn_on — Turn on lights"));
        assert!(out.contains("light.turn_off — Turn off lights"));
        assert!(!out.contains("switch.toggle"));
    }

    // ── call_service guards ───────────────────────────────────────────────

    struct DenyAll;
    #[async_trait]
    impl Approver for DenyAll {
        async fn approve(&self, _request: &ApprovalRequest) -> bool {
            false
        }
    }

    struct AllowAll;
    #[async_trait]
    impl Approver for AllowAll {
        async fn approve(&self, _request: &ApprovalRequest) -> bool {
            true
        }
    }

    fn tool(approver: Arc<dyn Approver>) -> HomeAssistantTool {
        // Unreachable base_url: every test here is refused *before* any HTTP.
        HomeAssistantTool::new(
            "http://127.0.0.1:1".to_string(),
            "token".to_string(),
            approver,
        )
    }

    #[tokio::test]
    async fn call_service_blocked_domain_refused_even_when_approved() {
        // AllowAll proves the blocklist sits below approval: still refused.
        let out = tool(Arc::new(AllowAll))
            .execute(
                json!({"action": "call_service", "domain": "shell_command",
                       "service": "run", "data": {"command": "rm -rf /"}})
                .to_string(),
            )
            .await
            .unwrap();
        assert!(out.contains("blocked for security"));
    }

    #[tokio::test]
    async fn call_service_invalid_domain_refused() {
        let out = tool(Arc::new(AllowAll))
            .execute(
                json!({"action": "call_service", "domain": "../../api/config",
                       "service": "get"})
                .to_string(),
            )
            .await
            .unwrap();
        assert!(out.contains("invalid service domain"));
    }

    #[tokio::test]
    async fn call_service_rejected_when_approval_denied() {
        let out = tool(Arc::new(DenyAll))
            .execute(
                json!({"action": "call_service", "domain": "light",
                       "service": "turn_on", "entity_id": "light.kitchen"})
                .to_string(),
            )
            .await
            .unwrap();
        assert!(out.contains("rejected by user"));
    }

    #[tokio::test]
    async fn unknown_action_errors() {
        let err = tool(Arc::new(DenyAll))
            .execute(json!({"action": "bogus"}).to_string())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown action"));
    }
}
