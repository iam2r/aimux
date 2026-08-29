use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

use super::merge::{json_remove, json_set};
use super::protocol;
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
        required: true,
        default: None,
        storage: FieldStorage::Model,
    },
    protocol::PI_FIELD,
];

pub struct PiAdapter;

impl AgentAdapter for PiAdapter {
    fn id(&self) -> AppId {
        AppId::Pi
    }

    /// Drop a previously injected `providers."key"` subtree from models.json.
    fn clear_slot(&self, paths: &Paths, key: &str) -> Result<()> {
        let path = paths.pi_dir.join("models.json");
        if !path.is_file() {
            return Ok(());
        }
        let mut doc = read_models_json(&path)?;
        let existed = doc
            .get("providers")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|m| m.contains_key(key));
        if !existed {
            return Ok(());
        }
        json_remove(&mut doc, &["providers", key]).with_context(|| path.display().to_string())?;
        write_live_json(&path, &doc)
    }

    fn inspect(&self, paths: &Paths) -> Result<Option<super::LiveFinger>> {
        use super::LiveFinger;
        if !self.is_initialized(paths) {
            return Ok(None);
        }
        let settings_path = paths.pi_dir.join("settings.json");
        if !settings_path.exists() {
            return Ok(None);
        }
        let settings = read_settings_json(&settings_path)?;
        let slot = settings
            .get("defaultProvider")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let model = settings
            .get("defaultModel")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let base_url = read_models_json(&paths.pi_dir.join("models.json"))
            .ok()
            .and_then(|m| {
                m.get("providers")
                    .and_then(|p| p.get(&slot))
                    .and_then(|e| e.get("baseUrl"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default();
        Ok(Some(LiveFinger {
            slot_key: slot,
            base_url,
            model,
            native: false,
        }))
    }

    fn rescue(&self, paths: &Paths) -> Vec<super::RescuedRow> {
        use super::RescuedRow;
        let settings = || read_settings_json(&paths.pi_dir.join("settings.json")).ok();
        let Ok(models) = read_models_json(&paths.pi_dir.join("models.json")) else {
            return Vec::new();
        };
        let Some(providers) = models
            .get("providers")
            .and_then(serde_json::Value::as_object)
        else {
            return Vec::new();
        };
        let default_provider = settings()
            .and_then(|s| {
                s.get("defaultProvider")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_default();
        let default_model = settings().and_then(|s| {
            s.get("defaultModel")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        });

        let mut rows = Vec::new();
        for (key, entry) in providers {
            // OAuth-style entries carry no baseUrl to import.
            let Some(base_url) = entry
                .get("baseUrl")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let api_key = entry
                .get("apiKey")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let catalog: Vec<crate::store::ModelEntry> = entry
                .get("models")
                .and_then(serde_json::Value::as_array)
                .map(|m| {
                    // Pi's live rows use `name`; the store calls it `label`.
                    #[derive(serde::Deserialize)]
                    struct Row {
                        id: String,
                        #[serde(default, rename = "name")]
                        label: Option<String>,
                        #[serde(default, rename = "contextWindow")]
                        context_window: Option<u64>,
                        #[serde(default, rename = "maxTokens")]
                        max_tokens: Option<u64>,
                    }
                    m.iter()
                        .filter_map(|v| serde_json::from_value::<Row>(v.clone()).ok())
                        .map(|r| crate::store::ModelEntry {
                            id: r.id,
                            label: r.label,
                            context_window: r.context_window,
                            max_tokens: r.max_tokens,
                            target_model_id: None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let active = key == &default_provider;
            rows.push(RescuedRow {
                provider: crate::store::Provider {
                    id: String::new(),
                    name: key.clone(),
                    base_url: base_url.to_string(),
                    api_key,
                    model: if active { default_model.clone() } else { None },
                    catalog,
                    ..crate::store::Provider::blank(crate::store::AppId::Pi)
                },
                active,
            });
        }
        rows
    }

    fn display_name(&self) -> &'static str {
        "Pi"
    }

    fn fields(&self) -> &'static [FieldSpec] {
        FIELDS
    }

    fn resolved_dir(&self, paths: &Paths) -> PathBuf {
        paths.pi_dir.clone()
    }

    fn live_paths(&self, paths: &Paths) -> Vec<PathBuf> {
        let dir = self.resolved_dir(paths);
        vec![dir.join("models.json"), dir.join("settings.json")]
    }

    fn validate(&self, provider: &Provider) -> Result<()> {
        require_non_empty("name", &provider.name)?;
        require_non_empty("base_url", &provider.base_url)?;
        require_http_url(&provider.base_url)?;
        require_non_empty("api_key", &provider.api_key)?;
        require_non_empty("model", provider.model.as_deref().unwrap_or(""))?;
        let protocol = protocol::from_extras(&provider.extras)?;
        protocol::require_allowed(protocol, protocol::PI)?;
        Ok(())
    }

    fn model_ui(&self) -> super::models::ModelUi {
        super::models::ModelUi::Catalog {
            fields: super::models::PI_FIELDS,
        }
    }

    fn apply(&self, paths: &Paths, provider: &Provider) -> Result<ApplyOutcome> {
        if !self.is_initialized(paths) {
            return Ok(ApplyOutcome::SkippedUninitialized);
        }
        self.validate(provider)?;
        let model = provider
            .model
            .as_deref()
            .filter(|m| !m.is_empty())
            .ok_or_else(|| anyhow::anyhow!("model must not be empty"))?;

        let dir = self.resolved_dir(paths);
        let models_path = dir.join("models.json");
        let settings_path = dir.join("settings.json");
        let files = vec![models_path.clone(), settings_path.clone()];

        let original_models = read_existing_bytes(&models_path)?;
        let mut models_doc = parse_models_json(&models_path, original_models.as_deref())?;
        if let Some(snippet) = super::snippet_to_apply(provider) {
            self.apply_snippet(&mut models_doc, snippet);
        }
        patch_models(&mut models_doc, provider, model)
            .with_context(|| models_path.display().to_string())?;
        write_live_json(&models_path, &models_doc)?;

        if let Err(e) = write_settings(&settings_path, provider, model) {
            rollback_file(&models_path, original_models.as_deref()).with_context(|| {
                format!(
                    "failed to roll back {} after settings error",
                    models_path.display()
                )
            })?;
            return Err(e);
        }

        Ok(ApplyOutcome::Applied { files })
    }
}

fn patch_models(doc: &mut Value, provider: &Provider, model: &str) -> Result<()> {
    let protocol = protocol::from_extras(&provider.extras).unwrap_or(protocol::DEFAULT);
    let api = protocol::pi_api(protocol);
    // The slot key is the provider's display name; the entry's `name` field
    // mirrors it for display in Pi's UI.
    let key = provider.slot_key();
    json_set(
        doc,
        &["providers", key.as_str(), "name"],
        Value::String(provider.name.clone()),
    )?;
    json_set(
        doc,
        &["providers", key.as_str(), "baseUrl"],
        Value::String(provider.base_url.clone()),
    )?;
    json_set(
        doc,
        &["providers", key.as_str(), "api"],
        Value::String(api.to_string()),
    )?;
    json_set(
        doc,
        &["providers", key.as_str(), "apiKey"],
        Value::String(provider.api_key.clone()),
    )?;
    write_pi_models(doc, provider, model)
}

fn write_pi_models(doc: &mut Value, provider: &Provider, model_id: &str) -> Result<()> {
    let key = provider.slot_key();
    let slot = doc
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .and_then(|p| p.get_mut(key.as_str()))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow::anyhow!("expected object at providers.{key}"))?;
    if provider.catalog.is_empty() {
        if !slot.get("models").is_some_and(Value::is_array) {
            slot.insert("models".into(), Value::Array(Vec::new()));
        }
        let models = slot
            .get_mut("models")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| anyhow::anyhow!("expected array at providers.aimux.models"))?;
        if !models
            .iter()
            .any(|m| m.get("id").and_then(Value::as_str) == Some(model_id))
        {
            models.push(pi_model_value(&crate::store::ModelEntry {
                id: model_id.to_string(),
                ..crate::store::ModelEntry::default()
            }));
        }
        return Ok(());
    }
    let models: Vec<Value> = provider
        .catalog
        .iter()
        .filter(|row| !row.id.trim().is_empty())
        .map(pi_model_value)
        .collect();
    if models.is_empty() {
        slot.insert(
            "models".into(),
            Value::Array(vec![pi_model_value(&crate::store::ModelEntry {
                id: model_id.to_string(),
                ..crate::store::ModelEntry::default()
            })]),
        );
    } else {
        slot.insert("models".into(), Value::Array(models));
    }
    Ok(())
}

fn pi_model_value(row: &crate::store::ModelEntry) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), Value::String(row.id.clone()));
    if let Some(name) = row
        .label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        obj.insert("name".into(), Value::String(name.to_string()));
    }
    if let Some(n) = row.context_window {
        obj.insert("contextWindow".into(), serde_json::json!(n));
    }
    if let Some(n) = row.max_tokens {
        obj.insert("maxTokens".into(), serde_json::json!(n));
    }
    Value::Object(obj)
}

fn write_settings(path: &Path, provider: &Provider, model: &str) -> Result<()> {
    let mut doc = read_settings_json(path)?;
    json_set(
        &mut doc,
        &["defaultProvider"],
        Value::String(provider.slot_key()),
    )
    .with_context(|| path.display().to_string())?;
    json_set(
        &mut doc,
        &["defaultModel"],
        Value::String(model.to_string()),
    )
    .with_context(|| path.display().to_string())?;
    write_live_json(path, &doc)
}

fn read_models_json(path: &Path) -> Result<Value> {
    if !path.exists() {
        let mut providers = serde_json::Map::new();
        providers.insert("providers".into(), Value::Object(serde_json::Map::new()));
        return Ok(Value::Object(providers));
    }
    let data = fs::read(path).map_err(|e| Error::io(path, e))?;
    let value: Value = serde_json::from_slice(&data).map_err(|e| Error::json(path, e))?;
    if !value.is_object() {
        anyhow::bail!("{}: root must be a JSON object", path.display());
    }
    Ok(value)
}

fn parse_models_json(path: &Path, existing: Option<&[u8]>) -> Result<Value> {
    let Some(data) = existing else {
        let mut providers = serde_json::Map::new();
        providers.insert("providers".into(), Value::Object(serde_json::Map::new()));
        return Ok(Value::Object(providers));
    };
    let value: Value = serde_json::from_slice(data).map_err(|e| Error::json(path, e))?;
    if !value.is_object() {
        anyhow::bail!("{}: root must be a JSON object", path.display());
    }
    match value.get("providers") {
        None => Ok(value),
        Some(p) if p.is_object() => Ok(value),
        Some(_) => anyhow::bail!("{}: providers must be a JSON object", path.display()),
    }
}

fn read_settings_json(path: &Path) -> Result<Value> {
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

fn read_existing_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(path, e).into()),
    }
}

fn rollback_file(path: &Path, original: Option<&[u8]>) -> Result<()> {
    match original {
        Some(bytes) => fsutil::atomic_write_preserving_mode(path, bytes),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::io(path, e).into()),
        },
    }
}

fn write_live_json(path: &Path, value: &Value) -> Result<()> {
    let mut body = serde_json::to_string_pretty(value).context("serialize live JSON")?;
    if !body.ends_with('\n') {
        body.push('\n');
    }
    fsutil::atomic_write_preserving_mode(path, body.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{get, parse_extras, registry};
    use crate::store::AppId;
    use std::collections::BTreeMap;

    fn setup() -> (tempfile::TempDir, Paths) {
        let td = tempfile::tempdir().expect("tempdir");
        let paths = Paths::for_test(td.path());
        (td, paths)
    }

    fn provider() -> Provider {
        Provider {
            id: "packy".into(),
            name: "PackyCode".into(),
            app: AppId::Pi,
            base_url: "https://proxy.example.com/v1".into(),
            api_key: "sk-test-key-abcd".into(),
            model: Some("claude-sonnet-4-5".into()),
            extras: BTreeMap::new(),
            ..Provider::blank(AppId::Pi)
        }
    }

    fn golden(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/pi")
            .join(name)
    }

    fn read_value(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn init(paths: &Paths) {
        fs::create_dir_all(&paths.pi_dir).unwrap();
    }

    #[test]
    fn resolved_dir_uses_paths_pi_dir() {
        let (_td, paths) = setup();
        let a = PiAdapter;
        assert_eq!(a.resolved_dir(&paths), paths.pi_dir);
        assert!(!a.is_initialized(&paths));
    }

    #[test]
    fn override_missing_is_not_initialized_even_if_home_pi_agent_exists() {
        let td = tempfile::tempdir().expect("tempdir");
        let home_agent = td.path().join(".pi").join("agent");
        fs::create_dir_all(&home_agent).unwrap();
        fs::write(home_agent.join("models.json"), b"{\"providers\":{}}\n").unwrap();
        fs::write(home_agent.join("settings.json"), b"{}\n").unwrap();
        let missing = td.path().join("override-pi");
        let paths = Paths::from_home_and_env(
            td.path().to_path_buf(),
            crate::paths::EnvOverrides {
                pi_coding_agent_dir: Some(missing.display().to_string()),
                ..crate::paths::EnvOverrides::default()
            },
        )
        .unwrap();
        let a = PiAdapter;
        assert_eq!(a.resolved_dir(&paths), missing);
        assert!(!a.is_initialized(&paths));
        let outcome = a.apply(&paths, &provider()).unwrap();
        assert_eq!(outcome, ApplyOutcome::SkippedUninitialized);
        assert!(!missing.exists());
        assert_eq!(
            fs::read_to_string(home_agent.join("models.json")).unwrap(),
            "{\"providers\":{}}\n"
        );
        assert_eq!(
            fs::read_to_string(home_agent.join("settings.json")).unwrap(),
            "{}\n"
        );
    }

    #[test]
    fn uninitialized_apply_does_not_create_dir() {
        let (_td, paths) = setup();
        let outcome = PiAdapter.apply(&paths, &provider()).unwrap();
        assert_eq!(outcome, ApplyOutcome::SkippedUninitialized);
        assert!(!paths.pi_dir.exists());
    }

    #[test]
    fn missing_files_treated_as_empty_new_models_0600() {
        let (_td, paths) = setup();
        init(&paths);
        let outcome = PiAdapter.apply(&paths, &provider()).unwrap();
        match outcome {
            ApplyOutcome::Applied { files } => {
                assert_eq!(
                    files,
                    vec![
                        paths.pi_dir.join("models.json"),
                        paths.pi_dir.join("settings.json")
                    ]
                );
            }
            other => panic!("{other:?}"),
        }
        let models = read_value(&paths.pi_dir.join("models.json"));
        let want_models = read_value(&golden("empty_models.after.json"));
        assert_eq!(models, want_models);
        let settings = read_value(&paths.pi_dir.join("settings.json"));
        let want_settings = read_value(&golden("empty_settings.after.json"));
        assert_eq!(settings, want_settings);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(paths.pi_dir.join("models.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn preserve_other_providers_and_extras() {
        let (_td, paths) = setup();
        init(&paths);
        let live = paths.pi_dir.join("models.json");
        fs::copy(golden("preserve_providers.before.json"), &live).unwrap();
        PiAdapter.apply(&paths, &provider()).unwrap();
        let got = read_value(&live);
        let want = read_value(&golden("preserve_providers.after.json"));
        assert_eq!(got, want);
    }

    #[test]
    fn preserve_extra_settings_keys() {
        let (_td, paths) = setup();
        init(&paths);
        let live = paths.pi_dir.join("settings.json");
        fs::copy(golden("preserve_settings.before.json"), &live).unwrap();
        PiAdapter.apply(&paths, &provider()).unwrap();
        let got = read_value(&live);
        let want = read_value(&golden("preserve_settings.after.json"));
        assert_eq!(got, want);
    }

    #[test]
    fn rescue_reads_models_and_settings() {
        let (_td, paths) = setup();
        std::fs::create_dir_all(&paths.pi_dir).unwrap();
        std::fs::write(
            paths.pi_dir.join("models.json"),
            r#"{"providers":{
                "Agate":{"baseUrl":"https://agate.example.com/v1","api":"anthropic-messages","apiKey":"sk-a",
                  "models":[{"id":"mimo-v2.5","name":"MiMo","contextWindow":500000}]},
                "oauth-only":{"api":"openai-completions"}
            }}"#,
        )
        .unwrap();
        std::fs::write(
            paths.pi_dir.join("settings.json"),
            r#"{"defaultProvider":"Agate","defaultModel":"mimo-v2.5"}"#,
        )
        .unwrap();
        let rows = PiAdapter.rescue(&paths);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].active);
        let p = &rows[0].provider;
        assert_eq!(p.name, "Agate");
        assert_eq!(p.api_key, "sk-a");
        assert_eq!(p.model.as_deref(), Some("mimo-v2.5"));
        assert_eq!(p.catalog.len(), 1);
        assert_eq!(p.catalog[0].label.as_deref(), Some("MiMo"));
        assert_eq!(p.catalog[0].context_window, Some(500000));
    }

    #[test]
    fn clear_slot_removes_named_provider() {
        let (_td, paths) = setup();
        std::fs::create_dir_all(&paths.pi_dir).unwrap();
        let live = paths.pi_dir.join("models.json");
        std::fs::write(
            &live,
            br#"{"providers":{"ollama":{"baseUrl":"http://x"},"P":{"baseUrl":"https://y"}}}"#,
        )
        .unwrap();
        PiAdapter.clear_slot(&paths, "P").unwrap();
        let doc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&live).unwrap()).unwrap();
        assert!(doc["providers"].get("P").is_none());
        assert!(doc["providers"].get("ollama").is_some());
    }

    #[test]
    fn appends_model_id_without_dropping_others() {
        let (_td, paths) = setup();
        init(&paths);
        let live = paths.pi_dir.join("models.json");
        fs::copy(golden("preserve_providers.before.json"), &live).unwrap();
        let mut p = provider();
        p.model = Some("gpt-4o".into());
        PiAdapter.apply(&paths, &p).unwrap();
        // Appending the model id keeps the rows already in the slot.
        let models = &read_value(&live)["providers"]["PackyCode"]["models"];
        assert_eq!(models[0]["id"], "claude-sonnet-4-5");
        assert_eq!(models[0]["name"], "Sonnet");
        assert_eq!(models[1]["id"], "keep-me");
        assert_eq!(models[2]["id"], "gpt-4o");
        assert_eq!(models.as_array().unwrap().len(), 3);
    }

    #[test]
    fn catalog_replaces_models_array() {
        use crate::store::ModelEntry;
        let (_td, paths) = setup();
        init(&paths);
        let live = paths.pi_dir.join("models.json");
        fs::copy(golden("preserve_providers.before.json"), &live).unwrap();
        let mut p = provider();
        p.catalog = vec![ModelEntry {
            id: "m1".into(),
            label: Some("One".into()),
            context_window: Some(10),
            max_tokens: Some(2),
            target_model_id: None,
        }];
        PiAdapter.apply(&paths, &p).unwrap();
        let models = &read_value(&live)["providers"]["PackyCode"]["models"];
        assert_eq!(models.as_array().unwrap().len(), 1);
        assert_eq!(models[0]["id"], "m1");
        assert_eq!(models[0]["name"], "One");
        assert_eq!(models[0]["contextWindow"], 10);
        assert_eq!(models[0]["maxTokens"], 2);
    }

    #[test]
    fn extra_api_written_unknown_extras_ignored() {
        let (_td, paths) = setup();
        init(&paths);
        let mut p = provider();
        p.extras.insert("protocol".into(), "anthropic".into());
        p.extras.insert("nope".into(), "xyz".into());
        PiAdapter.validate(&p).unwrap();
        PiAdapter.apply(&paths, &p).unwrap();
        let models = read_value(&paths.pi_dir.join("models.json"));
        assert_eq!(
            models["providers"]["PackyCode"]["api"],
            "anthropic-messages"
        );
        assert!(models["providers"]["PackyCode"].get("nope").is_none());
        assert!(models.get("nope").is_none());
        p.extras.remove("protocol");
        p.extras.insert("api".into(), "google-generative-ai".into());
        PiAdapter.apply(&paths, &p).unwrap();
        let models = read_value(&paths.pi_dir.join("models.json"));
        assert_eq!(
            models["providers"]["PackyCode"]["api"],
            "google-generative-ai"
        );
    }

    #[test]
    fn does_not_write_auth_json() {
        let (_td, paths) = setup();
        init(&paths);
        let auth = paths.pi_dir.join("auth.json");
        fs::write(&auth, b"{\"anthropic\":{\"type\":\"oauth\"}}\n").unwrap();
        PiAdapter.apply(&paths, &provider()).unwrap();
        assert_eq!(
            fs::read_to_string(&auth).unwrap(),
            "{\"anthropic\":{\"type\":\"oauth\"}}\n"
        );
    }

    #[test]
    fn project_dot_pi_untouched() {
        let (td, paths) = setup();
        init(&paths);
        let project = td.path().join("my-repo").join(".pi");
        fs::create_dir_all(&project).unwrap();
        let proj_settings = project.join("settings.json");
        let proj_models = project.join("models.json");
        let proj_settings_bytes = fs::read(golden("project.settings.json")).unwrap();
        let proj_models_bytes = fs::read(golden("project.models.json")).unwrap();
        fs::write(&proj_settings, &proj_settings_bytes).unwrap();
        fs::write(&proj_models, &proj_models_bytes).unwrap();
        let home_pi_settings = paths.home.join(".pi").join("settings.json");
        fs::write(&home_pi_settings, b"{\"not-agent\":true}\n").unwrap();
        let crate_pi = Path::new(env!("CARGO_MANIFEST_DIR")).join(".pi");
        let crate_pi_existed = crate_pi.exists();

        PiAdapter.apply(&paths, &provider()).unwrap();

        assert_eq!(fs::read(&proj_settings).unwrap(), proj_settings_bytes);
        assert_eq!(fs::read(&proj_models).unwrap(), proj_models_bytes);
        assert_eq!(
            fs::read_to_string(&home_pi_settings).unwrap(),
            "{\"not-agent\":true}\n"
        );
        if !crate_pi_existed {
            assert!(
                !crate_pi.exists(),
                "must not write project .pi/ under the crate root"
            );
        }
    }

    #[test]
    fn corrupt_models_writes_nothing() {
        let (_td, paths) = setup();
        init(&paths);
        let models = paths.pi_dir.join("models.json");
        let settings = paths.pi_dir.join("settings.json");
        let bytes = fs::read(golden("corrupt.json")).unwrap();
        fs::write(&models, &bytes).unwrap();
        fs::write(&settings, b"{}\n").unwrap();
        let err = PiAdapter.apply(&paths, &provider()).unwrap_err();
        assert!(
            err.to_string().contains("models.json"),
            "error should name the path: {err}"
        );
        assert_eq!(fs::read(&models).unwrap(), bytes);
        assert_eq!(fs::read(&settings).unwrap(), b"{}\n");
    }

    #[test]
    fn providers_not_object_writes_nothing() {
        let (_td, paths) = setup();
        init(&paths);
        let models = paths.pi_dir.join("models.json");
        let settings = paths.pi_dir.join("settings.json");
        let bytes = br#"{"providers":[]}"#;
        fs::write(&models, bytes).unwrap();
        fs::write(&settings, b"{}\n").unwrap();
        let err = PiAdapter.apply(&paths, &provider()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("providers must be a JSON object"), "{msg}");
        assert!(msg.contains("models.json"), "{msg}");
        assert_eq!(fs::read(&models).unwrap(), bytes);
        assert_eq!(fs::read(&settings).unwrap(), b"{}\n");
    }

    #[test]
    fn missing_providers_key_creates_object() {
        let (_td, paths) = setup();
        init(&paths);
        let models = paths.pi_dir.join("models.json");
        fs::write(&models, b"{\"unrelated\":true}\n").unwrap();
        PiAdapter.apply(&paths, &provider()).unwrap();
        let doc = read_value(&models);
        assert_eq!(doc["unrelated"], true);
        assert_eq!(doc["providers"]["PackyCode"]["apiKey"], "sk-test-key-abcd");
        assert_eq!(
            doc["providers"]["PackyCode"]["models"][0]["id"],
            "claude-sonnet-4-5"
        );
    }

    #[test]
    fn corrupt_settings_rolls_back_models() {
        let (_td, paths) = setup();
        init(&paths);
        let models = paths.pi_dir.join("models.json");
        let settings = paths.pi_dir.join("settings.json");
        let original = fs::read(golden("preserve_providers.before.json")).unwrap();
        fs::write(&models, &original).unwrap();
        let corrupt = fs::read(golden("corrupt.json")).unwrap();
        fs::write(&settings, &corrupt).unwrap();
        let err = PiAdapter.apply(&paths, &provider()).unwrap_err();
        assert!(
            err.to_string().contains("settings.json"),
            "error should name the path: {err}"
        );
        assert_eq!(fs::read(&models).unwrap(), original);
        assert_eq!(fs::read(&settings).unwrap(), corrupt);
        crate::fsutil::panic_if_host_config_path(&models);
    }

    #[test]
    fn settings_not_object_rolls_back_new_models_file() {
        let (_td, paths) = setup();
        init(&paths);
        let models = paths.pi_dir.join("models.json");
        let settings = paths.pi_dir.join("settings.json");
        fs::write(&settings, b"[]").unwrap();
        let err = PiAdapter.apply(&paths, &provider()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("root must be a JSON object"), "{msg}");
        assert!(msg.contains("settings.json"), "{msg}");
        assert!(!models.exists());
        assert_eq!(fs::read(&settings).unwrap(), b"[]");
    }

    #[test]
    fn second_write_failure_rolls_back_models() {
        let (_td, paths) = setup();
        init(&paths);
        let models = paths.pi_dir.join("models.json");
        let settings = paths.pi_dir.join("settings.json");
        let original = fs::read(golden("preserve_providers.before.json")).unwrap();
        fs::write(&models, &original).unwrap();
        fs::write(&settings, b"{}\n").unwrap();
        fsutil::fail_before_rename_nth(2);
        let err = PiAdapter.apply(&paths, &provider()).unwrap_err();
        assert!(err.to_string().contains("injected failure"), "error: {err}");
        assert_eq!(fs::read(&models).unwrap(), original);
        assert_eq!(fs::read(&settings).unwrap(), b"{}\n");
        crate::fsutil::panic_if_host_config_path(&models);
    }

    #[test]
    fn validate_rejects_empty_model_and_bad_api() {
        let mut p = provider();
        p.model = None;
        assert!(PiAdapter.validate(&p).is_err());
        p = provider();
        p.model = Some(String::new());
        assert!(PiAdapter.validate(&p).is_err());
        p = provider();
        p.name.clear();
        assert!(PiAdapter.validate(&p).is_err());
        p = provider();
        p.base_url = "ftp://x".into();
        assert!(PiAdapter.validate(&p).is_err());
        p = provider();
        p.api_key.clear();
        assert!(PiAdapter.validate(&p).is_err());
        p = provider();
        p.extras.insert("api".into(), "neither".into());
        assert!(PiAdapter.validate(&p).is_err());
        p = provider();
        p.extras.insert("api".into(), "openai-responses".into());
        PiAdapter.validate(&p).unwrap();
        p.extras.insert("api".into(), "google-generative-ai".into());
        PiAdapter.validate(&p).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn existing_live_perms_preserved() {
        use std::os::unix::fs::PermissionsExt;
        let (_td, paths) = setup();
        init(&paths);
        let models = paths.pi_dir.join("models.json");
        fs::write(&models, b"{\"providers\":{}}\n").unwrap();
        fs::set_permissions(&models, fs::Permissions::from_mode(0o644)).unwrap();
        let settings = paths.pi_dir.join("settings.json");
        fs::write(&settings, b"{}\n").unwrap();
        fs::set_permissions(&settings, fs::Permissions::from_mode(0o644)).unwrap();
        PiAdapter.apply(&paths, &provider()).unwrap();
        let mode = fs::metadata(&models).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
        let mode = fs::metadata(&settings).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
    }

    #[test]
    fn isolation_apply_does_not_touch_host() {
        let (_td, paths) = setup();
        init(&paths);
        PiAdapter.apply(&paths, &provider()).unwrap();
        crate::fsutil::panic_if_host_config_path(&paths.pi_dir.join("models.json"));
        crate::fsutil::panic_if_host_config_path(&paths.pi_dir.join("settings.json"));
        let host = dirs::home_dir().expect("home").join(".pi");
        assert_ne!(paths.pi_dir, host);
        assert_ne!(paths.pi_dir, host.join("agent"));
    }

    #[test]
    fn registry_pi_fields() {
        let a = get(AppId::Pi).unwrap();
        assert_eq!(a.display_name(), "Pi");
        assert_eq!(a.fields().len(), 5);
        let protocol = a.fields().iter().find(|f| f.key == "protocol").unwrap();
        assert!(matches!(protocol.kind, FieldKind::Select(_)));
        assert!(!protocol.required);
        assert!(a.fields().iter().any(|f| f.key == "model" && f.required));
        assert!(registry().iter().any(|x| x.id() == AppId::Pi));
        let ok = parse_extras(a, &["protocol=openai-responses".into()]).unwrap();
        assert_eq!(
            ok.get("protocol").map(String::as_str),
            Some("openai-responses")
        );
        let err = parse_extras(a, &["protocol=nope".into()]).unwrap_err();
        assert!(err.to_string().contains("invalid value"));
        let err = parse_extras(a, &["api=openai-responses".into()]).unwrap_err();
        assert!(err.to_string().contains("unknown extra field"));
    }
}
