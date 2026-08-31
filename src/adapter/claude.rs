use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::Value;

use super::merge::{json_remove, json_set};
use super::{
    require_http_url, require_non_empty, AgentAdapter, AppId, ApplyOutcome, FieldKind, FieldSpec,
    FieldStorage,
};
use crate::error::Error;
use crate::fsutil;
use crate::paths::Paths;
use crate::store::Provider;

const FIELDS: &[FieldSpec] = &[
    FieldSpec {
        key: "name",
        label: "field.name",
        kind: FieldKind::Text,
        required: true,
        default: None,
        storage: FieldStorage::Name,
    },
    FieldSpec {
        key: "base_url",
        label: "field.base_url",
        kind: FieldKind::Url,
        required: true,
        default: None,
        storage: FieldStorage::BaseUrl,
    },
    FieldSpec {
        key: "api_key",
        label: "field.api_key",
        kind: FieldKind::Secret,
        required: true,
        default: None,
        storage: FieldStorage::ApiKey,
    },
    FieldSpec {
        key: "model",
        label: "field.model",
        kind: FieldKind::Model,
        required: false,
        default: None,
        storage: FieldStorage::Model,
    },
    FieldSpec {
        key: "api_key_field",
        label: "field.api_key_field",
        kind: FieldKind::Select(&["auth_token", "api_key"]),
        required: false,
        default: Some("auth_token"),
        storage: FieldStorage::Extra("api_key_field"),
    },
];

pub struct ClaudeAdapter;

impl ClaudeAdapter {
    fn live_file(&self, paths: &Paths) -> PathBuf {
        let dir = self.resolved_dir(paths);
        let settings = dir.join("settings.json");
        let legacy = dir.join("claude.json");
        if !settings.exists() && legacy.exists() {
            legacy
        } else {
            settings
        }
    }
}

impl AgentAdapter for ClaudeAdapter {
    fn id(&self) -> AppId {
        AppId::Claude
    }

    fn display_name(&self) -> &'static str {
        "Claude"
    }

    fn fields(&self) -> &'static [FieldSpec] {
        FIELDS
    }

    fn resolved_dir(&self, paths: &Paths) -> PathBuf {
        paths.claude_dir.clone()
    }

    fn live_paths(&self, paths: &Paths) -> Vec<PathBuf> {
        vec![self.live_file(paths)]
    }

    fn validate(&self, provider: &Provider) -> Result<()> {
        require_non_empty("name", &provider.name)?;
        if provider.official {
            return Ok(());
        }
        require_non_empty("base_url", &provider.base_url)?;
        require_http_url(&provider.base_url)?;
        require_non_empty("api_key", &provider.api_key)?;
        if let Some(v) = provider.extras.get("api_key_field") {
            if v != "auth_token" && v != "api_key" {
                anyhow::bail!("invalid api_key_field: {v}");
            }
        }
        Ok(())
    }

    fn inspect(&self, paths: &Paths) -> Result<Option<super::LiveFinger>> {
        use super::LiveFinger;
        if !self.is_initialized(paths) {
            return Ok(None);
        }
        let doc = read_json_object(&self.live_file(paths))?;
        let env = doc.get("env").and_then(serde_json::Value::as_object);
        let str_field = |k: &str| {
            env.and_then(|e| e.get(k))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let base_url = str_field("ANTHROPIC_BASE_URL").unwrap_or_default();
        let has_key =
            str_field("ANTHROPIC_AUTH_TOKEN").is_some() || str_field("ANTHROPIC_API_KEY").is_some();
        // No injected material at all: Claude's native login is in charge.
        // (A lone AUTH_TOKEN without our base URL is not ours either — it
        // falls through as an unattributable injection.)
        Ok(Some(LiveFinger {
            slot_key: String::new(),
            base_url,
            model: str_field("ANTHROPIC_MODEL").unwrap_or_default(),
            native: !has_key,
        }))
    }

    fn rescue(&self, paths: &Paths) -> Vec<super::RescuedRow> {
        use super::RescuedRow;
        let path = self.live_file(paths);
        let Ok(doc) = read_json_object(&path) else {
            return Vec::new();
        };
        let Some(env) = doc.get("env").and_then(serde_json::Value::as_object) else {
            return Vec::new();
        };
        let str_field = |k: &str| {
            env.get(k)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        // A hand-configured user has a third-party base URL set.
        let Some(base_url) = str_field("ANTHROPIC_BASE_URL") else {
            return Vec::new();
        };
        let api_key = str_field("ANTHROPIC_AUTH_TOKEN")
            .or_else(|| str_field("ANTHROPIC_API_KEY"))
            .unwrap_or_default();
        let mut slots = std::collections::BTreeMap::new();
        for spec in super::models::CLAUDE_SLOTS {
            if let Some(v) = str_field(spec.env_key) {
                slots.insert(spec.key.to_string(), v);
            }
        }
        vec![RescuedRow {
            provider: crate::store::Provider {
                id: "claude".into(),
                name: "Claude".into(),
                base_url,
                api_key,
                model: str_field("ANTHROPIC_MODEL"),
                slots,
                ..crate::store::Provider::blank(crate::store::AppId::Claude)
            },
            active: true,
        }]
    }

    fn model_ui(&self) -> super::models::ModelUi {
        super::models::ModelUi::Catalog {
            fields: super::models::CLAUDE_FIELDS,
        }
    }

    fn quick_items(&self) -> &'static [super::quick::QuickItem] {
        super::quick::CLAUDE
    }

    fn apply(&self, paths: &Paths, provider: &Provider) -> Result<ApplyOutcome> {
        if !self.is_initialized(paths) {
            return Ok(ApplyOutcome::SkippedUninitialized);
        }
        self.validate(provider)?;

        let files = self.live_paths(paths);
        let live = files
            .first()
            .ok_or_else(|| anyhow::anyhow!("claude adapter has no live path"))?;
        let mut doc = read_json_object(live)?;
        if let Some(snippet) = super::snippet_to_apply(provider) {
            self.apply_snippet(&mut doc, snippet);
        }
        patch_claude_env(&mut doc, provider).with_context(|| live.display().to_string())?;
        write_live_json(live, &doc)?;
        Ok(ApplyOutcome::Applied { files })
    }
}

fn patch_claude_env(doc: &mut Value, provider: &Provider) -> Result<()> {
    // The official row hands Claude Code back to its native subscription:
    // strip every apmux-owned key so its own login takes over.
    if provider.official {
        for key in [
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_MODEL",
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
        ] {
            json_remove(doc, &["env", key])?;
        }
        for slot in super::models::CLAUDE_SLOTS {
            json_remove(doc, &["env", slot.env_key])?;
        }
        json_remove(doc, &["modelOverrides"])?;
        return Ok(());
    }

    json_set(
        doc,
        &["env", "ANTHROPIC_BASE_URL"],
        Value::String(provider.base_url.clone()),
    )?;

    let use_api_key = provider.extras.get("api_key_field").map(String::as_str) == Some("api_key");
    let key = Value::String(provider.api_key.clone());
    if use_api_key {
        json_set(doc, &["env", "ANTHROPIC_API_KEY"], key)?;
        json_remove(doc, &["env", "ANTHROPIC_AUTH_TOKEN"])?;
    } else {
        json_set(doc, &["env", "ANTHROPIC_AUTH_TOKEN"], key)?;
        json_remove(doc, &["env", "ANTHROPIC_API_KEY"])?;
    }

    // Helper: a slot env value (or ANTHROPIC_MODEL). If the bound row has a
    // `target_model_id` that Claude Code recognises, route through the
    // Anthropic ID and let `modelOverrides` translate the actual id later.
    // Otherwise write the proxy id directly (the unknown-model warning path).
    let resolve_id = |row_id: &str| -> String {
        let trimmed = row_id.trim();
        provider
            .catalog
            .iter()
            .find(|m| m.id.trim() == trimmed)
            .and_then(|m| m.target_model_id.as_deref())
            .map(str::trim)
            .filter(|tid| !tid.is_empty() && super::models::is_known_claude_model_id(tid))
            .map(str::to_string)
            .unwrap_or_else(|| trimmed.to_string())
    };

    match &provider.model {
        Some(model) => {
            let resolved = resolve_id(model);
            json_set(doc, &["env", "ANTHROPIC_MODEL"], Value::String(resolved))?;
        }
        None => json_remove(doc, &["env", "ANTHROPIC_MODEL"])?,
    }
    for slot in super::models::CLAUDE_SLOTS {
        match provider
            .slots
            .get(slot.key)
            .map(String::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(id) => {
                let resolved = resolve_id(id);
                json_set(doc, &["env", slot.env_key], Value::String(resolved))?
            }
            None => json_remove(doc, &["env", slot.env_key])?,
        }
    }

    // `CLAUDE_CODE_MAX_CONTEXT_TOKENS`: min over all catalog rows with a
    // non-empty `context_window`. None/all-empty → key absent.
    let min_window = provider
        .catalog
        .iter()
        .filter_map(|m| m.context_window)
        .min();
    match min_window {
        Some(w) => json_set(
            doc,
            &["env", "CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            Value::String(w.to_string()),
        )?,
        None => json_remove(doc, &["env", "CLAUDE_CODE_MAX_CONTEXT_TOKENS"])?,
    }

    // `modelOverrides`: one entry per catalog row whose `target_model_id`
    // is in `KNOWN_CLAUDE_MODEL_IDS`. Key is the Anthropic ID (so Claude
    // Code accepts it); value is the row's actual proxy id. Rows without
    // a valid target model id are skipped — they stay plain unknown ids.
    let mut overrides = serde_json::Map::new();
    for row in &provider.catalog {
        let Some(target) = row.target_model_id.as_deref().map(str::trim) else {
            continue;
        };
        if !super::models::is_known_claude_model_id(target) {
            continue;
        }
        let row_id = row.id.trim();
        if row_id.is_empty() {
            continue;
        }
        // Last-wins on duplicate targets across rows; matches the natural
        // "later row overrides earlier" reading of a catalog.
        overrides.insert(target.to_string(), Value::String(row_id.to_string()));
    }
    if overrides.is_empty() {
        json_remove(doc, &["modelOverrides"])?;
    } else {
        json_set(doc, &["modelOverrides"], Value::Object(overrides))?;
    }

    Ok(())
}

fn read_json_object(path: &std::path::Path) -> Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    let data = fs::read(path).map_err(|e| Error::io(path, e))?;
    let value: Value = serde_json::from_slice(&data).map_err(|e| Error::json(path, e))?;
    if !value.is_object() {
        anyhow::bail!("{}: root must be a JSON object", path.display());
    }
    Ok(value)
}

fn write_live_json(path: &std::path::Path, value: &Value) -> Result<()> {
    let mut body = serde_json::to_string_pretty(value).context("serialize live JSON")?;
    if !body.ends_with('\n') {
        body.push('\n');
    }
    fsutil::atomic_write_preserving_mode(path, body.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{get, registry};
    use crate::store::AppId;
    use crate::store::ModelEntry;
    use std::collections::BTreeMap;
    use std::path::Path;

    fn setup() -> (tempfile::TempDir, Paths) {
        let td = tempfile::tempdir().expect("tempdir");
        let paths = Paths::for_test(td.path());
        (td, paths)
    }

    #[test]
    fn rescue_reads_hand_configured_env() {
        let (td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        fs::write(
            paths.claude_dir.join("settings.json"),
            r#"{"env":{
                "ANTHROPIC_BASE_URL":"https://agate.example.com",
                "ANTHROPIC_AUTH_TOKEN":"sk-agate",
                "ANTHROPIC_MODEL":"claude-sonnet-4-5",
                "ANTHROPIC_DEFAULT_OPUS_MODEL":"opus-x",
                "ANTHROPIC_SMALL_FAST_MODEL":"haiku-mini"
            }}"#,
        )
        .unwrap();
        let rows = ClaudeAdapter.rescue(&paths);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].active);
        let p = &rows[0].provider;
        assert_eq!(p.base_url, "https://agate.example.com");
        assert_eq!(p.api_key, "sk-agate");
        assert_eq!(p.model.as_deref(), Some("claude-sonnet-4-5"));
        // Slot keys map into the slots table; unrelated env is ignored.
        assert_eq!(p.slots.get("opus").map(String::as_str), Some("opus-x"));
        assert!(!p.slots.contains_key("sonnet"));

        // Native-login setups (no third-party base URL) are not adopted.
        fs::write(
            paths.claude_dir.join("settings.json"),
            r#"{"env":{"FOO":"bar"}}"#,
        )
        .unwrap();
        assert!(ClaudeAdapter.rescue(&paths).is_empty());
        drop(td);
    }

    fn provider(model: Option<&str>) -> Provider {
        Provider {
            id: "packy".into(),
            name: "PackyCode".into(),
            app: AppId::Claude,
            base_url: "https://api.example.com".into(),
            api_key: "sk-test-key-abcd".into(),
            model: model.map(str::to_string),
            extras: BTreeMap::new(),
            ..Provider::blank(AppId::Claude)
        }
    }

    #[test]
    fn official_apply_strips_anthropic_env() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let live = paths.claude_dir.join("settings.json");
        fs::write(
            &live,
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://third.party","ANTHROPIC_AUTH_TOKEN":"sk-x","CLAUDE_CODE_MAX_CONTEXT_TOKENS":"999","OTHER":"keep"},"modelOverrides":{"haiku":"x"}}"#,
        )
        .unwrap();
        let official = crate::store::official_provider(AppId::Claude).unwrap();
        ClaudeAdapter.apply(&paths, &official).unwrap();
        let got: Value = serde_json::from_str(&fs::read_to_string(&live).unwrap()).unwrap();
        for key in [
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_MODEL",
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
            "CLAUDE_CODE_SUBAGENT_MODEL",
        ] {
            assert!(got["env"].get(key).is_none(), "{key} should be stripped");
        }
        assert!(
            got.get("modelOverrides").is_none(),
            "modelOverrides should be stripped"
        );
        assert_eq!(got["env"]["OTHER"], "keep"); // unrelated keys survive

        // Switching back to a third-party provider writes its env again.
        ClaudeAdapter.apply(&paths, &provider(None)).unwrap();
        let got: Value = serde_json::from_str(&fs::read_to_string(&live).unwrap()).unwrap();
        assert_eq!(got["env"]["ANTHROPIC_BASE_URL"], "https://api.example.com");
    }

    fn golden(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/claude")
            .join(name)
    }

    fn read_value(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn resolved_dir_uses_paths_claude_dir() {
        let (_td, paths) = setup();
        let a = ClaudeAdapter;
        assert_eq!(a.resolved_dir(&paths), paths.claude_dir);
        assert!(!a.is_initialized(&paths));
    }

    #[test]
    fn isolated_claude_json_does_not_count_as_initialized() {
        let (_td, paths) = setup();
        fs::write(paths.home.join(".claude.json"), b"{}").unwrap();
        assert!(!ClaudeAdapter.is_initialized(&paths));
        assert!(!paths.claude_dir.is_dir());
    }

    #[test]
    fn override_missing_is_not_initialized_even_if_home_claude_exists() {
        let td = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(td.path().join(".claude")).unwrap();
        fs::write(td.path().join(".claude").join("settings.json"), b"{}").unwrap();
        let missing = td.path().join("override-claude");
        let paths = Paths::from_home_and_env(
            td.path().to_path_buf(),
            crate::paths::EnvOverrides {
                claude_config_dir: Some(missing.display().to_string()),
                ..crate::paths::EnvOverrides::default()
            },
        )
        .unwrap();
        let a = ClaudeAdapter;
        assert_eq!(a.resolved_dir(&paths), missing);
        assert!(!a.is_initialized(&paths));
        let outcome = a.apply(&paths, &provider(None)).unwrap();
        assert_eq!(outcome, ApplyOutcome::SkippedUninitialized);
        assert!(!missing.exists());
        assert_eq!(
            fs::read_to_string(td.path().join(".claude").join("settings.json")).unwrap(),
            "{}"
        );
    }

    #[test]
    fn uninitialized_apply_does_not_create_dir() {
        let (_td, paths) = setup();
        let a = ClaudeAdapter;
        let outcome = a.apply(&paths, &provider(None)).unwrap();
        assert_eq!(outcome, ApplyOutcome::SkippedUninitialized);
        assert!(!paths.claude_dir.exists());
    }

    #[test]
    fn missing_settings_treated_as_empty_new_file_0600() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let a = ClaudeAdapter;
        let outcome = a.apply(&paths, &provider(Some("sonnet"))).unwrap();
        match outcome {
            ApplyOutcome::Applied { files } => {
                assert_eq!(files, vec![paths.claude_dir.join("settings.json")]);
            }
            other => panic!("{other:?}"),
        }
        let live = paths.claude_dir.join("settings.json");
        let got = read_value(&live);
        let want = read_value(&golden("empty.after.json"));
        assert_eq!(got, want);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&live).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn writes_legacy_claude_json_when_only_legacy_exists() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let legacy = paths.claude_dir.join("claude.json");
        fs::write(&legacy, b"{}\n").unwrap();
        ClaudeAdapter.apply(&paths, &provider(None)).unwrap();
        assert!(legacy.exists());
        assert!(!paths.claude_dir.join("settings.json").exists());
        let doc = read_value(&legacy);
        assert_eq!(doc["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-test-key-abcd");
    }

    #[test]
    fn preserve_unrelated_settings() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let live = paths.claude_dir.join("settings.json");
        fs::copy(golden("preserve_unrelated.before.json"), &live).unwrap();
        ClaudeAdapter.apply(&paths, &provider(None)).unwrap();
        let got = read_value(&live);
        let want = read_value(&golden("preserve_unrelated.after.json"));
        assert_eq!(got, want);
    }

    #[test]
    fn token_mutex_auth_token_deletes_api_key() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let live = paths.claude_dir.join("settings.json");
        fs::copy(golden("token_mutex.before.json"), &live).unwrap();
        ClaudeAdapter.apply(&paths, &provider(None)).unwrap();
        let got = read_value(&live);
        let want = read_value(&golden("token_mutex.after.json"));
        assert_eq!(got, want);
    }

    #[test]
    fn token_mutex_api_key_deletes_auth_token() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let live = paths.claude_dir.join("settings.json");
        fs::copy(golden("token_mutex_api_key.before.json"), &live).unwrap();
        let mut p = provider(None);
        p.extras.insert("api_key_field".into(), "api_key".into());
        ClaudeAdapter.apply(&paths, &p).unwrap();
        let got = read_value(&live);
        let want = read_value(&golden("token_mutex_api_key.after.json"));
        assert_eq!(got, want);
    }

    #[test]
    fn model_none_deletes_live_key() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let live = paths.claude_dir.join("settings.json");
        fs::copy(golden("model_none.before.json"), &live).unwrap();
        ClaudeAdapter.apply(&paths, &provider(None)).unwrap();
        let got = read_value(&live);
        let want = read_value(&golden("model_none.after.json"));
        assert_eq!(got, want);
        assert!(got["env"].get("ANTHROPIC_MODEL").is_none());
    }

    #[test]
    fn corrupt_json_writes_nothing() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let live = paths.claude_dir.join("settings.json");
        let bytes = fs::read(golden("corrupt.json")).unwrap();
        fs::write(&live, &bytes).unwrap();
        let err = ClaudeAdapter.apply(&paths, &provider(None)).unwrap_err();
        assert!(
            err.to_string().contains("settings.json"),
            "error should name the path: {err}"
        );
        assert_eq!(fs::read(&live).unwrap(), bytes);
    }

    #[test]
    fn env_not_object_writes_nothing() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let live = paths.claude_dir.join("settings.json");
        let bytes = br#"{"env":"not-an-object"}"#;
        fs::write(&live, bytes).unwrap();
        let err = ClaudeAdapter.apply(&paths, &provider(None)).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("expected object at env"), "{msg}");
        assert!(
            msg.contains("settings.json"),
            "structure error must name the live path: {msg}"
        );
        assert_eq!(fs::read(&live).unwrap(), bytes);
    }

    #[test]
    fn unknown_extras_ignored() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut p = provider(None);
        p.extras.insert("nope".into(), "xyz".into());
        ClaudeAdapter.validate(&p).unwrap();
        ClaudeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.claude_dir.join("settings.json"));
        assert!(doc["env"].get("nope").is_none());
        assert!(doc.get("nope").is_none());
    }

    #[test]
    fn empty_slots_do_not_project_role_models() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        ClaudeAdapter
            .apply(&paths, &provider(Some("sonnet")))
            .unwrap();
        let doc = read_value(&paths.claude_dir.join("settings.json"));
        let env = doc["env"].as_object().unwrap();
        assert!(!env.contains_key("ANTHROPIC_DEFAULT_SONNET_MODEL"));
        assert!(!env.contains_key("ANTHROPIC_DEFAULT_HAIKU_MODEL"));
        assert!(!env.contains_key("ANTHROPIC_DEFAULT_OPUS_MODEL"));
    }

    #[test]
    fn slots_write_role_env_keys_and_ignore_unknown() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut p = provider(Some("sonnet-default"));
        p.slots.insert("haiku".into(), "haiku-id".into());
        p.slots.insert("opus".into(), "opus-id".into());
        p.slots.insert("nope".into(), "ignored".into());
        ClaudeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.claude_dir.join("settings.json"));
        assert_eq!(doc["env"]["ANTHROPIC_MODEL"], "sonnet-default");
        assert_eq!(doc["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "haiku-id");
        assert_eq!(doc["env"]["ANTHROPIC_DEFAULT_OPUS_MODEL"], "opus-id");
        assert!(doc["env"].get("ANTHROPIC_DEFAULT_SONNET_MODEL").is_none());
        assert!(doc["env"].get("nope").is_none());
    }

    #[test]
    fn teammates_quick_item_writes_env() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let item = crate::adapter::quick::CLAUDE
            .iter()
            .find(|i| i.id == "teammates")
            .unwrap();
        let mut snippet = serde_json::json!({});
        item.apply_snippet(&mut snippet);
        let mut p = provider(None);
        p.snippet = Some(snippet);
        p.apply_snippet = true;
        ClaudeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.claude_dir.join("settings.json"));
        assert_eq!(doc["env"]["CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"], "1");
    }

    #[test]
    fn snippet_merges_then_owned_fields_win() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut p = provider(None);
        p.snippet = Some(serde_json::json!({
            "includeCoAuthoredBy": false,
            "env": {
                "FOO": "bar",
                "ANTHROPIC_BASE_URL": "https://from-snippet.example"
            }
        }));
        p.apply_snippet = true;
        ClaudeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.claude_dir.join("settings.json"));
        assert_eq!(doc["includeCoAuthoredBy"], false);
        assert_eq!(doc["env"]["FOO"], "bar");
        assert_eq!(doc["env"]["ANTHROPIC_BASE_URL"], "https://api.example.com");
    }

    #[test]
    fn validate_rejects_empty_and_bad_url() {
        let mut p = provider(None);
        p.name.clear();
        assert!(ClaudeAdapter.validate(&p).is_err());
        p = provider(None);
        p.base_url = "ftp://x".into();
        assert!(ClaudeAdapter.validate(&p).is_err());
        p = provider(None);
        p.api_key.clear();
        assert!(ClaudeAdapter.validate(&p).is_err());
        p = provider(None);
        p.extras.insert("api_key_field".into(), "neither".into());
        assert!(ClaudeAdapter.validate(&p).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn existing_live_perms_preserved() {
        use std::os::unix::fs::PermissionsExt;
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let live = paths.claude_dir.join("settings.json");
        fs::write(&live, b"{}\n").unwrap();
        fs::set_permissions(&live, fs::Permissions::from_mode(0o644)).unwrap();
        ClaudeAdapter.apply(&paths, &provider(None)).unwrap();
        let mode = fs::metadata(&live).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
    }

    #[test]
    fn isolation_apply_does_not_touch_host() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        ClaudeAdapter.apply(&paths, &provider(None)).unwrap();
        crate::fsutil::panic_if_host_config_path(&paths.claude_dir.join("settings.json"));
        let host = dirs::home_dir().expect("home").join(".claude");
        assert_ne!(paths.claude_dir, host);
    }

    #[test]
    fn registry_claude_fields() {
        let a = get(AppId::Claude).unwrap();
        assert_eq!(a.display_name(), "Claude");
        assert_eq!(a.fields().len(), 5);
        assert!(registry().iter().any(|x| x.id() == AppId::Claude));
    }

    #[allow(dead_code)]
    fn provider_with_catalog(catalog: Vec<ModelEntry>) -> Provider {
        let mut p = provider(None);
        p.catalog = catalog;
        p
    }

    #[test]
    fn max_context_tokens_min_over_catalog() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut p = provider(Some("m"));
        p.catalog = vec![
            ModelEntry {
                id: "m".into(),
                context_window: Some(200_000),
                target_model_id: None,
                ..ModelEntry::default()
            },
            ModelEntry {
                id: "x".into(),
                context_window: Some(1_000_000),
                target_model_id: None,
                ..ModelEntry::default()
            },
            ModelEntry {
                id: "y".into(),
                context_window: None, // ignored
                target_model_id: None,
                ..ModelEntry::default()
            },
        ];
        ClaudeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.claude_dir.join("settings.json"));
        assert_eq!(doc["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "200000");
    }

    #[test]
    fn max_context_tokens_absent_when_catalog_empty() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        ClaudeAdapter.apply(&paths, &provider(None)).unwrap();
        let doc = read_value(&paths.claude_dir.join("settings.json"));
        assert!(doc["env"].get("CLAUDE_CODE_MAX_CONTEXT_TOKENS").is_none());
    }

    #[test]
    fn target_model_id_writes_anthropic_id_for_known_targets() {
        // Row x is the default AND has target_model_id = claude-sonnet-4-6.
        // ANTHROPIC_MODEL is rewritten to the Anthropic ID, and the
        // modelOverrides entry maps that Anthropic ID back to the proxy id.
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut p = provider(Some("x"));
        p.catalog = vec![ModelEntry {
            id: "x".into(),
            context_window: None,
            target_model_id: Some("claude-sonnet-4-6".into()),
            ..ModelEntry::default()
        }];
        ClaudeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.claude_dir.join("settings.json"));
        assert_eq!(doc["env"]["ANTHROPIC_MODEL"], "claude-sonnet-4-6");
        assert_eq!(doc["modelOverrides"]["claude-sonnet-4-6"], "x");
    }

    #[test]
    fn slot_env_uses_target_model_id_when_known() {
        // Row "x" is bound to sonnet+opus; its target is claude-opus-4-7.
        // Each slot env value becomes the Anthropic ID; modelOverrides
        // is keyed by that ID.
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut p = provider(None);
        p.slots.insert("sonnet".into(), "x".into());
        p.slots.insert("opus".into(), "x".into());
        p.catalog = vec![ModelEntry {
            id: "x".into(),
            context_window: None,
            target_model_id: Some("claude-opus-4-7".into()),
            ..ModelEntry::default()
        }];
        ClaudeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.claude_dir.join("settings.json"));
        assert_eq!(
            doc["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "claude-opus-4-7"
        );
        assert_eq!(
            doc["env"]["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            "claude-opus-4-7"
        );
        assert_eq!(doc["modelOverrides"]["claude-opus-4-7"], "x");
    }

    #[test]
    fn unknown_target_model_id_falls_back_to_proxy_id() {
        // target_model_id is a string but not in KNOWN_CLAUDE_MODEL_IDS —
        // the env value should stay as the proxy id, and modelOverrides
        // must NOT include the entry (unknown keys are ignored by Claude
        // Code, and writing them is just dead config).
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut p = provider(Some("x"));
        p.catalog = vec![ModelEntry {
            id: "x".into(),
            context_window: None,
            target_model_id: Some("totally-made-up-id".into()),
            ..ModelEntry::default()
        }];
        ClaudeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.claude_dir.join("settings.json"));
        assert_eq!(doc["env"]["ANTHROPIC_MODEL"], "x");
        assert!(doc.get("modelOverrides").is_none());
    }

    #[test]
    fn model_overrides_absent_when_no_target_set() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut p = provider(Some("x"));
        p.slots.insert("haiku".into(), "x".into());
        p.catalog = vec![ModelEntry {
            id: "x".into(),
            context_window: None,
            target_model_id: None,
            ..ModelEntry::default()
        }];
        ClaudeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.claude_dir.join("settings.json"));
        // ANTHROPIC_MODEL and slot env fall back to the proxy id;
        // modelOverrides stays absent.
        assert_eq!(doc["env"]["ANTHROPIC_MODEL"], "x");
        assert_eq!(doc["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "x");
        assert!(doc.get("modelOverrides").is_none());
    }

    #[test]
    fn model_overrides_one_entry_per_known_target() {
        // Two rows with different known targets → two modelOverrides keys.
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut p = provider(Some("a"));
        p.slots.insert("haiku".into(), "b".into());
        p.catalog = vec![
            ModelEntry {
                id: "a".into(),
                context_window: None,
                target_model_id: Some("claude-sonnet-4-6".into()),
                ..ModelEntry::default()
            },
            ModelEntry {
                id: "b".into(),
                context_window: None,
                target_model_id: Some("claude-opus-4-7".into()),
                ..ModelEntry::default()
            },
        ];
        ClaudeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.claude_dir.join("settings.json"));
        assert_eq!(doc["modelOverrides"]["claude-sonnet-4-6"], "a");
        assert_eq!(doc["modelOverrides"]["claude-opus-4-7"], "b");
        assert_eq!(doc["env"]["ANTHROPIC_MODEL"], "claude-sonnet-4-6");
        assert_eq!(
            doc["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"],
            "claude-opus-4-7"
        );
    }

    #[test]
    fn model_overrides_duplicate_target_is_last_wins() {
        // Two rows pointing at the same known target → the later row's id
        // wins in `modelOverrides`. Matches the "later row overrides earlier"
        // reading of a catalog (see docs §4.3 duplicate-target note).
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut p = provider(None);
        p.slots.insert("haiku".into(), "second".into());
        p.catalog = vec![
            ModelEntry {
                id: "first".into(),
                context_window: None,
                target_model_id: Some("claude-sonnet-4-6".into()),
                ..ModelEntry::default()
            },
            ModelEntry {
                id: "second".into(),
                context_window: None,
                target_model_id: Some("claude-sonnet-4-6".into()),
                ..ModelEntry::default()
            },
        ];
        ClaudeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.claude_dir.join("settings.json"));
        // Last-wins: only one entry per target, and it points to "second".
        let mo = &doc["modelOverrides"];
        assert_eq!(mo.as_object().unwrap().len(), 1);
        assert_eq!(mo["claude-sonnet-4-6"], "second");
        // The haiku env value is also routed through the second row's id.
        assert_eq!(
            doc["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"],
            "claude-sonnet-4-6"
        );
    }

    #[test]
    fn max_context_tokens_stripped_when_switching_back() {
        // After a third-party provider with MAX_CONTEXT_TOKENS, switching
        // back to a clean third-party provider without one removes the key.
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut p1 = provider(Some("m"));
        p1.catalog = vec![ModelEntry {
            id: "m".into(),
            context_window: Some(500_000),
            target_model_id: None,
            ..ModelEntry::default()
        }];
        ClaudeAdapter.apply(&paths, &p1).unwrap();
        let doc1 = read_value(&paths.claude_dir.join("settings.json"));
        assert_eq!(doc1["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "500000");

        let p2 = provider(None);
        ClaudeAdapter.apply(&paths, &p2).unwrap();
        let doc2 = read_value(&paths.claude_dir.join("settings.json"));
        assert!(doc2["env"].get("CLAUDE_CODE_MAX_CONTEXT_TOKENS").is_none());
    }
}
