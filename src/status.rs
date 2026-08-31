//! `apmux status` — reconcile the store against each app's live config.
//!
//! `current` reports what the store *recorded*; status reads the live files
//! back ([`AgentAdapter::inspect`]) and compares, so drift introduced by
//! hand edits or other tools becomes visible.

use crate::adapter::{registry, LiveFinger};
use crate::paths::Paths;
use crate::store::{AppId, Provider, Store};
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Live config matches the store's current provider.
    Ok,
    /// Live points at a known provider (or native login) that differs from
    /// the store's current row, or its fields diverge from store records.
    Drift,
    /// Live carries an injection apmux cannot attribute to any stored row.
    External,
    /// The app's native login is in charge and the store agrees (official).
    Native,
    /// App not initialized on this machine.
    Missing,
}

impl State {
    fn label(self) -> &'static str {
        match self {
            State::Ok => "ok",
            State::Drift => "drift",
            State::External => "external",
            State::Native => "native",
            State::Missing => "missing",
        }
    }
}

/// Where the active key material lives for an app.
fn key_source(app: AppId) -> &'static str {
    match app {
        AppId::Claude => "settings.json env",
        AppId::Codex => "auth.json",
        AppId::OpenCode => "opencode.json",
        AppId::Pi => "models.json",
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub app: AppId,
    pub state: State,
    /// Resolved provider name (store attribution), when known.
    pub provider_name: Option<String>,
    /// Model observed live.
    pub model: String,
    /// Human hint appended after STATE, e.g. "store says axl".
    pub detail: String,
}

pub fn collect(paths: &Paths, store: &Store) -> Vec<AppState> {
    let mut rows = Vec::new();
    for adapter in registry() {
        let app = adapter.id();
        if !adapter.is_initialized(paths) {
            rows.push(AppState {
                app,
                state: State::Missing,
                provider_name: None,
                model: String::new(),
                detail: String::new(),
            });
            continue;
        }
        let finger = match adapter.inspect(paths) {
            Ok(Some(f)) => f,
            _ => {
                rows.push(AppState {
                    app,
                    state: State::Missing,
                    provider_name: None,
                    model: String::new(),
                    detail: "unreadable".into(),
                });
                continue;
            }
        };
        rows.push(reconcile(app, store, &finger));
    }
    rows
}

fn current_provider(store: &Store, app: AppId) -> Option<&Provider> {
    store
        .current
        .get(&app)
        .and_then(|id| store.providers.get(id))
}

fn find_by_name<'a>(store: &'a Store, app: AppId, slot_key: &str) -> Option<&'a Provider> {
    // Slot keys are display names; same-app names are unique by construction.
    store
        .providers
        .values()
        .find(|p| p.app == app && p.slot_key() == slot_key)
}

fn reconcile(app: AppId, store: &Store, finger: &LiveFinger) -> AppState {
    let cur = current_provider(store, app);
    let detail_store = |owner: Option<&Provider>| -> String {
        match (cur, owner) {
            (Some(c), Some(o)) if c.id != o.id => format!("store says {}", c.name),
            (None, Some(_)) => "no store record".into(),
            _ => String::new(),
        }
    };

    if finger.native {
        // Native login is expected only when the store's current row is the
        // official one (or there is no record at all yet).
        match cur {
            None => {
                return AppState {
                    app,
                    state: State::Native,
                    provider_name: None,
                    model: String::new(),
                    detail: String::new(),
                }
            }
            Some(c) if c.official => {
                return AppState {
                    app,
                    state: State::Ok,
                    provider_name: Some(c.name.clone()),
                    model: String::new(),
                    detail: String::new(),
                }
            }
            Some(c) => {
                return AppState {
                    app,
                    state: State::Drift,
                    provider_name: Some(c.name.clone()),
                    model: String::new(),
                    detail: format!("live is native login; store says {}", c.name),
                }
            }
        }
    }

    // Attribute the injection to a stored row.
    let owner = find_by_name(store, app, &finger.slot_key).or_else(|| {
        // Apps without slot identity (claude): fall back to matching the
        // recorded current row's base_url, then any unique base_url hit.
        let mut hits = store.providers.values().filter(|p| {
            p.app == app && !finger.base_url.is_empty() && p.base_url == finger.base_url
        });
        let first = hits.next()?;
        if hits.next().is_some() {
            return None;
        }
        Some(first)
    });

    let Some(owner) = owner else {
        return AppState {
            app,
            state: State::External,
            provider_name: None,
            model: finger.model.clone(),
            detail: if finger.slot_key.is_empty() {
                format!("base_url {}", finger.base_url)
            } else {
                format!("slot {}", finger.slot_key)
            },
        };
    };

    // Field-level reconciliation.
    let mut diffs: Vec<String> = Vec::new();
    if !finger.base_url.is_empty()
        && !owner.base_url.is_empty()
        && finger.base_url != owner.base_url
    {
        diffs.push("base_url changed".into());
    }
    if !finger.model.is_empty()
        && owner.model.as_deref().unwrap_or_default() != finger.model
        && cur.map(|c| c.id.as_str()) != Some(owner.id.as_str())
    {
        diffs.push("model differs from store".into());
    }

    let is_current = cur.map(|c| c.id == owner.id).unwrap_or(false);
    let state = if is_current && diffs.is_empty() {
        State::Ok
    } else {
        State::Drift
    };
    let mut detail = detail_store(Some(owner));
    if let Some(first) = diffs.first() {
        if !detail.is_empty() {
            detail.push_str("; ");
        }
        detail.push_str(first);
    }
    AppState {
        app,
        state,
        provider_name: Some(owner.name.clone()),
        model: finger.model.clone(),
        detail,
    }
}

// ---------------------------------------------------------------- rendering

const W_APP: usize = 10;
const W_PROVIDER: usize = 18;
const W_MODEL: usize = 14;
const W_KEYSRC: usize = 16;

pub fn render(rows: &[AppState], json: bool, show_secrets: bool) -> Result<String> {
    // Keys are never echoed in status output — identity, not secrets.
    let _ = show_secrets;
    if json {
        let values: Vec<serde_json::Value> = rows.iter().map(state_json).collect();
        let mut out = serde_json::to_string_pretty(&values)?;
        if !out.ends_with('\n') {
            out.push('\n');
        }
        return Ok(out);
    }
    let mut out = String::new();
    out.push_str(&format!(
        "{:<W_APP$}  {:<W_PROVIDER$}  {:<W_MODEL$}  {:<W_KEYSRC$}  STATE\n",
        "APP", "PROVIDER", "MODEL", "KEY SOURCE"
    ));
    for r in rows {
        let provider = r.provider_name.clone().unwrap_or_else(|| match r.state {
            State::Missing => "(not installed)".into(),
            State::Native => "Claude Official / official".into(),
            State::External => "(external)".into(),
            _ => "-".into(),
        });
        let provider = truncate(&provider, W_PROVIDER - 1);
        let model_raw = if r.state == State::Missing {
            "-".to_string()
        } else {
            r.model.clone()
        };
        let model = truncate(&model_raw, W_MODEL - 1);
        let key_src = match r.state {
            State::Missing | State::External => "-".into(),
            State::Native => "native login".into(),
            _ => key_source(r.app).to_string(),
        };
        // AppId's Display uses write_str, which ignores width padding — go
        // through String so the APP column actually aligns.
        let app_label = r.app.to_string();
        out.push_str(&format!(
            "{:<W_APP$}  {:<W_PROVIDER$}  {:<W_MODEL$}  {:<W_KEYSRC$}  {}\n",
            app_label,
            provider,
            model,
            key_src,
            if r.detail.is_empty() {
                r.state.label().to_string()
            } else {
                format!("{} ({})", r.state.label(), r.detail)
            }
        ));
    }
    Ok(out)
}

fn state_json(r: &AppState) -> serde_json::Value {
    serde_json::json!({
        "app": r.app.to_string(),
        "state": r.state.label(),
        "provider": r.provider_name,
        "model": r.model,
        "key_source": key_source(r.app),
        "detail": r.detail,
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut cut: String = s.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::fs;
    use std::path::Path;

    fn temp() -> (tempfile::TempDir, Paths) {
        let td = tempfile::tempdir().unwrap();
        let paths = Paths::for_test(td.path());
        (td, paths)
    }

    fn third_party(app: AppId, name: &str) -> Provider {
        Provider {
            id: format!("{app}-{name}"),
            name: name.into(),
            app,
            base_url: "https://api.example.com".into(),
            api_key: "sk-test-key-abcd".into(),
            model: Some("gpt-5.2".into()),
            ..Provider::blank(app)
        }
    }

    fn mark_current(store: &mut Store, app: AppId, name: &str) {
        let id = format!("{app}-{name}");
        store.current.insert(app, id.clone());
        // slot_keys records the display name written into live config.
        if let Some(p) = store.providers.get(&id) {
            store.slot_keys.insert(app, p.slot_key());
        }
    }

    fn write(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn missing_when_not_initialized() {
        let (_td, paths) = temp();
        let store = Store::empty();
        for row in collect(&paths, &store) {
            assert_eq!(row.state, State::Missing);
        }
    }

    #[test]
    fn codex_ok_when_live_matches_store() {
        let (_td, paths) = temp();
        fs::create_dir_all(&paths.codex_dir).unwrap();
        let mut store = Store::empty();
        store
            .providers
            .insert("codex-pk".into(), third_party(AppId::Codex, "pk"));
        mark_current(&mut store, AppId::Codex, "pk");
        write(
            &paths.codex_dir.join("config.toml"),
            "model_provider = \"pk\"\nmodel = \"gpt-5.2\"\n\n[model_providers.pk]\nname = \"pk\"\nbase_url = \"https://api.example.com\"\n",
        );
        write(
            &paths.codex_dir.join("auth.json"),
            "{\"OPENAI_API_KEY\":\"sk-test-key-abcd\"}",
        );

        let rows = collect(&paths, &store);
        let codex = rows.iter().find(|r| r.app == AppId::Codex).unwrap();
        assert_eq!(codex.state, State::Ok, "{codex:?}");
        assert_eq!(codex.provider_name.as_deref(), Some("pk"));
        assert_eq!(codex.model, "gpt-5.2");
    }

    #[test]
    fn codex_native_is_ok_only_for_official_current() {
        let (_td, paths) = temp();
        fs::create_dir_all(&paths.codex_dir).unwrap();
        write(
            &paths.codex_dir.join("config.toml"),
            "model = \"o4-mini\"\n",
        );

        // No record yet → Native.
        let empty = Store::empty();
        let rows = collect(&paths, &empty);
        let codex = rows.iter().find(|r| r.app == AppId::Codex).unwrap();
        assert_eq!(codex.state, State::Native);

        // Store points at a third-party row while live is native → Drift.
        let mut store = Store::empty();
        store
            .providers
            .insert("codex-pk".into(), third_party(AppId::Codex, "pk"));
        mark_current(&mut store, AppId::Codex, "pk");
        let rows = collect(&paths, &store);
        let codex = rows.iter().find(|r| r.app == AppId::Codex).unwrap();
        assert_eq!(codex.state, State::Drift);
        assert!(codex.detail.contains("native"), "{codex:?}");
    }

    #[test]
    fn codex_external_when_slot_unknown() {
        let (_td, paths) = temp();
        fs::create_dir_all(&paths.codex_dir).unwrap();
        let mut store = Store::empty();
        store
            .providers
            .insert("codex-pk".into(), third_party(AppId::Codex, "pk"));
        mark_current(&mut store, AppId::Codex, "pk");
        write(
            &paths.codex_dir.join("config.toml"),
            "model_provider = \"mystery\"\n\n[model_providers.mystery]\nbase_url = \"https://mystery.io/v1\"\n",
        );
        let rows = collect(&paths, &store);
        let codex = rows.iter().find(|r| r.app == AppId::Codex).unwrap();
        assert_eq!(codex.state, State::External, "{codex:?}");
        assert!(codex.detail.contains("mystery"), "{codex:?}");
    }

    #[test]
    fn claude_attribution_falls_back_to_base_url_match() {
        let (_td, paths) = temp();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut store = Store::empty();
        let mut p = third_party(AppId::Claude, "relay");
        p.base_url = "https://relay.example.com".into();
        store.providers.insert(p.id.clone(), p);
        mark_current(&mut store, AppId::Claude, "relay");
        // Claude writes no slot key; only env fields identify the owner.
        write(
            &paths.claude_dir.join("settings.json"),
            "{\"env\":{\"ANTHROPIC_BASE_URL\":\"https://relay.example.com\",\"ANTHROPIC_AUTH_TOKEN\":\"sk-x\",\"ANTHROPIC_MODEL\":\"claude-opus-4-5\"}}",
        );
        let rows = collect(&paths, &store);
        let claude = rows.iter().find(|r| r.app == AppId::Claude).unwrap();
        assert_eq!(claude.state, State::Ok, "{claude:?}");
        assert_eq!(claude.provider_name.as_deref(), Some("relay"));

        // Point env at a foreign URL → External.
        write(
            &paths.claude_dir.join("settings.json"),
            "{\"env\":{\"ANTHROPIC_BASE_URL\":\"https://stranger.io\",\"ANTHROPIC_AUTH_TOKEN\":\"sk-y\"}}",
        );
        let rows = collect(&paths, &store);
        let claude = rows.iter().find(|r| r.app == AppId::Claude).unwrap();
        assert_eq!(claude.state, State::External, "{claude:?}");
    }

    #[test]
    fn opencode_and_pi_report_slot_and_model() {
        let (_td, paths) = temp();
        fs::create_dir_all(&paths.opencode_dir).unwrap();
        fs::create_dir_all(&paths.pi_dir).unwrap();

        let oc = third_party(AppId::OpenCode, "oc-relay");
        write(
            &paths.opencode_dir.join("opencode.json"),
            &format!(
                "{{\"provider\":{{\"{}\":{{\"options\":{{\"baseURL\":\"https://api.example.com\"}}}}}},\"model\":\"{}/gpt-5.2\"}}",
                Provider { name: "oc-relay".into(), ..Provider::blank(AppId::OpenCode) }.slot_key(),
                Provider { name: "oc-relay".into(), ..Provider::blank(AppId::OpenCode) }.slot_key()
            ),
        );

        let pi = third_party(AppId::Pi, "agate");
        write(
            &paths.pi_dir.join("models.json"),
            &format!(
                "{{\"providers\":{{\"{}\":{{\"baseUrl\":\"https://api.example.com\",\"apiKey\":\"sk-z\"}}}}}}",
                Provider { name: "agate".into(), ..Provider::blank(AppId::Pi) }.slot_key()
            ),
        );
        write(
            &paths.pi_dir.join("settings.json"),
            &format!(
                "{{\"defaultProvider\":\"{}\",\"defaultModel\":\"glm-4.7\"}}",
                Provider {
                    name: "agate".into(),
                    ..Provider::blank(AppId::Pi)
                }
                .slot_key()
            ),
        );

        let mut store = Store::empty();
        store.providers.insert(oc.id.clone(), oc);
        store.providers.insert(pi.id.clone(), pi);
        mark_current(&mut store, AppId::OpenCode, "oc-relay");
        mark_current(&mut store, AppId::Pi, "agate");

        let rows = collect(&paths, &store);
        let oc_row = rows.iter().find(|r| r.app == AppId::OpenCode).unwrap();
        assert_eq!(oc_row.state, State::Ok, "{oc_row:?}");
        assert_eq!(oc_row.model, "gpt-5.2");
        let pi_row = rows.iter().find(|r| r.app == AppId::Pi).unwrap();
        // defaultModel (glm-4.7) differs from provider.model (gpt-5.2) but the
        // live settings file IS the source of truth for Pi's active model —
        // reconcile only flags it when it disagrees with a *different* owner.
        assert_eq!(pi_row.provider_name.as_deref(), Some("agate"));
        assert_eq!(pi_row.model, "glm-4.7");
    }

    #[test]
    fn render_table_and_json() {
        let rows = vec![AppState {
            app: AppId::Codex,
            state: State::Drift,
            provider_name: Some("axl".into()),
            model: "gpt-5.2".into(),
            detail: "store says pk".into(),
        }];
        let table = render(&rows, false, false).unwrap();
        assert!(table.contains("drift (store says pk)"), "{table}");
        assert!(table.contains("auth.json"), "{table}");
        let json = render(&rows, true, false).unwrap();
        assert!(json.contains("\"state\": \"drift\""), "{json}");
        assert!(!json.contains("sk-"), "{json}");
    }
}
