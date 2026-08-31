use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{json, Value};

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

const SCHEMA_URL: &str = "https://opencode.ai/config.json";

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
    protocol::OPENCODE_FIELD,
];

pub struct OpenCodeAdapter;

impl OpenCodeAdapter {
    fn live_file(&self, paths: &Paths) -> PathBuf {
        self.resolved_dir(paths).join("opencode.json")
    }
}

impl AgentAdapter for OpenCodeAdapter {
    fn id(&self) -> AppId {
        AppId::OpenCode
    }

    /// Drop a previously injected `provider."key"` subtree from opencode.json.
    fn clear_slot(&self, paths: &Paths, key: &str) -> Result<()> {
        let path = self.live_file(paths);
        if !path.is_file() {
            return Ok(());
        }
        let mut doc = read_json_object(&path)?;
        let existed = doc
            .get("provider")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|m| m.contains_key(key));
        if !existed {
            return Ok(());
        }
        json_remove(&mut doc, &["provider", key]).with_context(|| path.display().to_string())?;
        write_live_json(&path, &doc)
    }

    fn inspect(&self, paths: &Paths) -> Result<Option<super::LiveFinger>> {
        use super::LiveFinger;
        if !self.is_initialized(paths) {
            return Ok(None);
        }
        let doc = read_json_object(&self.live_file(paths))?;
        // "model": "<slot>/<model-id>" names the active entry; without it the
        // config is not apmux-managed (OpenCode has no native-login mode).
        let Some(model_ref) = doc.get("model").and_then(serde_json::Value::as_str) else {
            return Ok(Some(LiveFinger {
                slot_key: String::new(),
                model: String::new(),
                base_url: String::new(),
                native: false,
            }));
        };
        let (slot, model_id) = model_ref.split_once('/').unwrap_or((model_ref, ""));
        let base_url = doc
            .get("provider")
            .and_then(|p| p.get(slot))
            .and_then(|e| e.get("options"))
            .and_then(|o| o.get("baseURL"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        Ok(Some(LiveFinger {
            slot_key: slot.to_string(),
            base_url,
            model: model_id.to_string(),
            native: false,
        }))
    }

    fn rescue(&self, paths: &Paths) -> Vec<super::RescuedRow> {
        use super::RescuedRow;
        let path = self.live_file(paths);
        let Ok(doc) = read_json_object(&path) else {
            return Vec::new();
        };
        let Some(providers) = doc.get("provider").and_then(serde_json::Value::as_object) else {
            return Vec::new();
        };
        // "model": "<slot>/<model-id>" names the active entry.
        let model_ref = doc
            .get("model")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();

        let mut rows = Vec::new();
        for (key, entry) in providers {
            // Built-in oauth-style entries have no baseURL to import.
            let Some(base_url) = entry
                .get("options")
                .and_then(|o| o.get("baseURL"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let api_key = entry
                .get("options")
                .and_then(|o| o.get("apiKey"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let mut catalog: Vec<crate::store::ModelEntry> = entry
                .get("models")
                .and_then(serde_json::Value::as_object)
                .map(|m| {
                    m.iter()
                        .map(|(id, v)| crate::store::ModelEntry {
                            id: id.clone(),
                            label: v
                                .get("name")
                                .and_then(serde_json::Value::as_str)
                                .filter(|s| !s.is_empty())
                                .map(str::to_string),
                            context_window: v
                                .pointer("/limit/context")
                                .and_then(serde_json::Value::as_u64),
                            max_tokens: v
                                .pointer("/limit/output")
                                .and_then(serde_json::Value::as_u64),
                            target_model_id: None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut model = None;
            let prefix = format!("{key}/");
            let active = model_ref.starts_with(&prefix);
            if active {
                model = Some(model_ref[prefix.len()..].to_string());
            }
            if catalog.is_empty() {
                catalog = model
                    .clone()
                    .map(|id| {
                        vec![crate::store::ModelEntry {
                            id,
                            ..Default::default()
                        }]
                    })
                    .unwrap_or_default();
            }
            rows.push(RescuedRow {
                provider: crate::store::Provider {
                    id: String::new(),
                    name: key.clone(),
                    base_url: base_url.to_string(),
                    api_key,
                    model,
                    catalog,
                    ..crate::store::Provider::blank(crate::store::AppId::OpenCode)
                },
                active,
            });
        }
        rows
    }

    fn display_name(&self) -> &'static str {
        "OpenCode"
    }

    fn fields(&self) -> &'static [FieldSpec] {
        FIELDS
    }

    fn resolved_dir(&self, paths: &Paths) -> PathBuf {
        paths.opencode_dir.clone()
    }

    fn live_paths(&self, paths: &Paths) -> Vec<PathBuf> {
        vec![self.live_file(paths)]
    }

    fn validate(&self, provider: &Provider) -> Result<()> {
        require_non_empty("name", &provider.name)?;
        require_non_empty("base_url", &provider.base_url)?;
        require_http_url(&provider.base_url)?;
        require_non_empty("api_key", &provider.api_key)?;
        require_non_empty("model", provider.model.as_deref().unwrap_or(""))?;
        let protocol = protocol::from_extras(&provider.extras)?;
        protocol::require_allowed(protocol, protocol::OPENCODE)?;
        Ok(())
    }

    fn model_ui(&self) -> super::models::ModelUi {
        super::models::ModelUi::Catalog {
            fields: super::models::OPENCODE_FIELDS,
        }
    }

    fn apply(&self, paths: &Paths, provider: &Provider) -> Result<ApplyOutcome> {
        if !self.is_initialized(paths) {
            return Ok(ApplyOutcome::SkippedUninitialized);
        }
        self.validate(provider)?;

        let files = self.live_paths(paths);
        let live = files
            .first()
            .ok_or_else(|| anyhow::anyhow!("opencode adapter has no live path"))?;
        let mut doc = read_json_object(live)?;
        if let Some(snippet) = super::snippet_to_apply(provider) {
            self.apply_snippet(&mut doc, snippet);
        }
        patch_opencode(&mut doc, provider).with_context(|| live.display().to_string())?;
        write_live_json(live, &doc)?;
        Ok(ApplyOutcome::Applied { files })
    }
}

fn patch_opencode(doc: &mut Value, provider: &Provider) -> Result<()> {
    let model_id = provider
        .model
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("model must not be empty"))?;
    // The slot key is the provider's display name; the entry's own fields
    // (npm/name/options/models) carry the rest of its identity.
    let key = provider.slot_key();
    json_set(doc, &["provider", key.as_str()], slot(provider, model_id))?;
    json_set(doc, &["model"], Value::String(format!("{key}/{model_id}")))?;
    Ok(())
}

fn slot(provider: &Provider, model_id: &str) -> Value {
    let protocol = protocol::from_extras(&provider.extras).unwrap_or(protocol::DEFAULT);
    let npm = protocol::opencode_npm(protocol);
    let mut models = serde_json::Map::new();
    let rows = if provider.catalog.is_empty() {
        vec![crate::store::ModelEntry {
            id: model_id.to_string(),
            ..crate::store::ModelEntry::default()
        }]
    } else {
        provider.catalog.clone()
    };
    for row in rows {
        let id = row.id.trim();
        if id.is_empty() {
            continue;
        }
        models.insert(id.to_string(), opencode_model_value(&row));
    }
    if models.is_empty() {
        models.insert(
            model_id.to_string(),
            opencode_model_value(&crate::store::ModelEntry {
                id: model_id.to_string(),
                ..crate::store::ModelEntry::default()
            }),
        );
    }
    json!({
        "npm": npm,
        "name": provider.name,
        "options": {
            "baseURL": provider.base_url,
            "apiKey": provider.api_key,
        },
        "models": models,
    })
}

fn opencode_model_value(row: &crate::store::ModelEntry) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "name".into(),
        json!(row
            .label
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&row.id)),
    );
    let mut limit = serde_json::Map::new();
    if let Some(n) = row.context_window {
        limit.insert("context".into(), json!(n));
    }
    if let Some(n) = row.max_tokens {
        limit.insert("output".into(), json!(n));
    }
    if !limit.is_empty() {
        obj.insert("limit".into(), Value::Object(limit));
    }
    Value::Object(obj)
}

fn read_json_object(path: &std::path::Path) -> Result<Value> {
    if !path.exists() {
        // Missing live file: OpenCode schema stub, then only owned paths.
        return Ok(json!({ "$schema": SCHEMA_URL }));
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
    use crate::adapter::{get, parse_extras, registry};
    use crate::store::AppId;
    use std::collections::BTreeMap;
    use std::path::Path;

    fn setup() -> (tempfile::TempDir, Paths) {
        let td = tempfile::tempdir().expect("tempdir");
        let paths = Paths::for_test(td.path());
        (td, paths)
    }

    fn provider(model: Option<&str>) -> Provider {
        Provider {
            id: "packy".into(),
            name: "PackyCode".into(),
            app: AppId::OpenCode,
            base_url: "https://api.example.com".into(),
            api_key: "sk-test-key-abcd".into(),
            model: model.map(str::to_string),
            extras: BTreeMap::new(),
            ..Provider::blank(AppId::OpenCode)
        }
    }

    fn golden(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/opencode")
            .join(name)
    }

    fn read_value(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn init_live(paths: &Paths) {
        fs::create_dir_all(&paths.opencode_dir).unwrap();
    }

    #[test]
    fn resolved_dir_uses_paths_opencode_dir() {
        let (_td, paths) = setup();
        let a = OpenCodeAdapter;
        assert_eq!(a.resolved_dir(&paths), paths.opencode_dir);
        assert!(!a.is_initialized(&paths));
        assert_eq!(
            a.live_paths(&paths),
            vec![paths.opencode_dir.join("opencode.json")]
        );
    }

    #[test]
    fn xdg_override_missing_does_not_write_home_config() {
        let td = tempfile::tempdir().expect("tempdir");
        let home_live_dir = td.path().join(".config").join("opencode");
        fs::create_dir_all(&home_live_dir).unwrap();
        let home_live = home_live_dir.join("opencode.json");
        fs::write(&home_live, b"{\"keep\":true}\n").unwrap();
        let missing = td.path().join("xdg-missing");
        let paths = Paths::from_home_and_env(
            td.path().to_path_buf(),
            crate::paths::EnvOverrides {
                xdg_config_home: Some(missing.display().to_string()),
                ..crate::paths::EnvOverrides::default()
            },
        )
        .unwrap();
        let a = OpenCodeAdapter;
        assert_eq!(a.resolved_dir(&paths), missing.join("opencode"));
        assert!(!a.is_initialized(&paths));
        let outcome = a.apply(&paths, &provider(Some("gpt-4o"))).unwrap();
        assert_eq!(outcome, ApplyOutcome::SkippedUninitialized);
        assert!(!missing.join("opencode").exists());
        assert_eq!(fs::read_to_string(&home_live).unwrap(), "{\"keep\":true}\n");
    }

    #[test]
    fn uninitialized_apply_does_not_create_dir() {
        let (_td, paths) = setup();
        let a = OpenCodeAdapter;
        let outcome = a.apply(&paths, &provider(Some("gpt-4o"))).unwrap();
        assert_eq!(outcome, ApplyOutcome::SkippedUninitialized);
        assert!(!paths.opencode_dir.exists());
    }

    #[test]
    fn missing_file_starts_from_schema_new_file_0600() {
        let (_td, paths) = setup();
        init_live(&paths);
        let outcome = OpenCodeAdapter
            .apply(&paths, &provider(Some("gpt-4o")))
            .unwrap();
        match outcome {
            ApplyOutcome::Applied { files } => {
                assert_eq!(files, vec![paths.opencode_dir.join("opencode.json")]);
            }
            other => panic!("{other:?}"),
        }
        let live = paths.opencode_dir.join("opencode.json");
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
    fn preserve_other_providers_mcp_and_unknown_keys() {
        let (_td, paths) = setup();
        init_live(&paths);
        let live = paths.opencode_dir.join("opencode.json");
        fs::copy(golden("preserve_providers.before.json"), &live).unwrap();
        OpenCodeAdapter
            .apply(&paths, &provider(Some("gpt-4o")))
            .unwrap();
        let got = read_value(&live);
        let want = read_value(&golden("preserve_providers.after.json"));
        assert_eq!(got, want);
        assert!(got["provider"].get("anthropic").is_some());
        assert!(got["provider"].get("openai").is_some());
        assert_eq!(got["mcp"]["filesystem"]["type"], "local");
        assert_eq!(got["unknownTop"]["keep"], true);
        assert_eq!(got["model"], "PackyCode/gpt-4o");
        assert!(got["provider"]["PackyCode"]["models"]
            .get("old-model")
            .is_none());
    }

    #[test]
    fn project_opencode_json_bytes_unchanged() {
        let (td, paths) = setup();
        init_live(&paths);
        let project_dir = td.path().join("project");
        fs::create_dir_all(&project_dir).unwrap();
        let project = project_dir.join("opencode.json");
        let agents = paths.opencode_dir.join("AGENTS.md");
        let stray = paths.home.join("opencode.json");
        let bytes = fs::read(golden("project.opencode.json")).unwrap();
        fs::write(&project, &bytes).unwrap();
        fs::write(&stray, &bytes).unwrap();
        fs::write(&agents, b"# keep\n").unwrap();
        OpenCodeAdapter
            .apply(&paths, &provider(Some("gpt-4o")))
            .unwrap();
        assert_eq!(fs::read(&project).unwrap(), bytes);
        assert_eq!(fs::read(&stray).unwrap(), bytes);
        assert_eq!(fs::read(&agents).unwrap(), b"# keep\n");
        assert!(paths.opencode_dir.join("opencode.json").exists());
    }

    #[test]
    fn model_required_validate_fails_no_write() {
        let (_td, paths) = setup();
        init_live(&paths);
        let live = paths.opencode_dir.join("opencode.json");
        let bytes = b"{\"keep\":true}\n";
        fs::write(&live, bytes).unwrap();
        let err = OpenCodeAdapter.validate(&provider(None)).unwrap_err();
        assert!(err.to_string().contains("model"), "{err}");
        let err = OpenCodeAdapter.apply(&paths, &provider(None)).unwrap_err();
        assert!(err.to_string().contains("model"), "{err}");
        assert_eq!(fs::read(&live).unwrap(), bytes);
        let mut empty = provider(Some(""));
        empty.model = Some(String::new());
        assert!(OpenCodeAdapter.validate(&empty).is_err());
    }

    #[test]
    fn corrupt_json_writes_nothing() {
        let (_td, paths) = setup();
        init_live(&paths);
        let live = paths.opencode_dir.join("opencode.json");
        let bytes = fs::read(golden("corrupt.json")).unwrap();
        fs::write(&live, &bytes).unwrap();
        let err = OpenCodeAdapter
            .apply(&paths, &provider(Some("gpt-4o")))
            .unwrap_err();
        assert!(
            err.to_string().contains("opencode.json"),
            "error should name the path: {err}"
        );
        assert_eq!(fs::read(&live).unwrap(), bytes);
    }

    #[test]
    fn provider_not_object_writes_nothing() {
        let (_td, paths) = setup();
        init_live(&paths);
        let live = paths.opencode_dir.join("opencode.json");
        let bytes = br#"{"provider":["not","an","object"]}"#;
        fs::write(&live, bytes).unwrap();
        let err = OpenCodeAdapter
            .apply(&paths, &provider(Some("gpt-4o")))
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("expected object at provider"), "{msg}");
        assert!(
            msg.contains("opencode.json"),
            "structure error must name the live path: {msg}"
        );
        assert_eq!(fs::read(&live).unwrap(), bytes);
    }

    #[test]
    fn protocol_maps_to_npm() {
        let (_td, paths) = setup();
        init_live(&paths);
        let mut p = provider(Some("gpt-4o"));
        p.extras.insert("protocol".into(), "anthropic".into());
        OpenCodeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.opencode_dir.join("opencode.json"));
        assert_eq!(doc["provider"]["PackyCode"]["npm"], "@ai-sdk/anthropic");
        p.extras
            .insert("protocol".into(), "openai-responses".into());
        OpenCodeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.opencode_dir.join("opencode.json"));
        assert_eq!(doc["provider"]["PackyCode"]["npm"], "@ai-sdk/openai");
        p.extras.insert("protocol".into(), "google".into());
        assert!(OpenCodeAdapter.validate(&p).is_err());
    }

    #[test]
    fn rescue_imports_providers_with_baseurl() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.opencode_dir).unwrap();
        fs::write(
            paths.opencode_dir.join("opencode.json"),
            r#"{
  "model": "Agate/mimo-v2.5",
  "provider": {
    "anthropic": { "npm": "@ai-sdk/anthropic" },
    "Agate": {
      "npm": "@ai-sdk/openai-compatible",
      "options": {
        "baseURL": "https://agate.example.com/v1",
        "apiKey": "sk-agate"
      },
      "models": {
        "mimo-v2.5": { "name": "MiMo", "limit": { "context": 500000, "output": 8192 } }
      }
    },
    "B": {
      "options": { "baseURL": "https://b.example.com/v1", "apiKey": "sk-b" }
    }
  }
}"#,
        )
        .unwrap();
        let rows = OpenCodeAdapter.rescue(&paths);
        assert_eq!(rows.len(), 2);

        let agate = rows.iter().find(|r| r.provider.name == "Agate").unwrap();
        assert!(agate.active);
        assert_eq!(agate.provider.base_url, "https://agate.example.com/v1");
        assert_eq!(agate.provider.api_key, "sk-agate");
        assert_eq!(agate.provider.model.as_deref(), Some("mimo-v2.5"));
        assert_eq!(agate.provider.catalog[0].id, "mimo-v2.5");
        assert_eq!(agate.provider.catalog[0].label.as_deref(), Some("MiMo"));
        assert_eq!(agate.provider.catalog[0].context_window, Some(500000));
        assert_eq!(agate.provider.catalog[0].max_tokens, Some(8192));

        let b = rows.iter().find(|r| r.provider.name == "B").unwrap();
        assert!(!b.active);
        // Inactive rows without a models table carry no catalog.
        assert_eq!(b.provider.catalog.len(), 0);
    }

    #[test]
    fn rescue_empty_without_config() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.opencode_dir).unwrap();
        assert!(OpenCodeAdapter.rescue(&paths).is_empty());
    }

    #[test]
    fn clear_slot_removes_only_named_provider() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.opencode_dir).unwrap();
        let live = paths.opencode_dir.join("opencode.json");
        fs::write(
            &live,
            r#"{"provider":{"A":{"options":{"baseURL":"https://a"}},"PackyCode":{"options":{"baseURL":"https://p"}}}}"#,
        )
        .unwrap();
        OpenCodeAdapter.clear_slot(&paths, "PackyCode").unwrap();
        let doc = read_value(&live);
        assert!(doc["provider"].get("PackyCode").is_none());
        assert!(doc["provider"].get("A").is_some());
    }

    #[test]
    fn catalog_writes_name_and_limits() {
        use crate::store::ModelEntry;
        let (_td, paths) = setup();
        init_live(&paths);
        let mut p = provider(Some("gpt-4o"));
        p.catalog = vec![
            ModelEntry {
                id: "gpt-4o".into(),
                label: Some("GPT-4o".into()),
                context_window: Some(128_000),
                max_tokens: Some(4096),
                target_model_id: None,
            },
            ModelEntry {
                id: "o3".into(),
                label: Some("o3".into()),
                context_window: Some(200_000),
                max_tokens: None,
                target_model_id: None,
            },
        ];
        OpenCodeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.opencode_dir.join("opencode.json"));
        assert_eq!(doc["model"], "PackyCode/gpt-4o");
        assert_eq!(
            doc["provider"]["PackyCode"]["models"]["gpt-4o"]["name"],
            "GPT-4o"
        );
        assert_eq!(
            doc["provider"]["PackyCode"]["models"]["gpt-4o"]["limit"]["context"],
            128000
        );
        assert_eq!(
            doc["provider"]["PackyCode"]["models"]["gpt-4o"]["limit"]["output"],
            4096
        );
        assert_eq!(
            doc["provider"]["PackyCode"]["models"]["o3"]["limit"]["context"],
            200000
        );
        assert!(doc["provider"]["PackyCode"]["models"]["o3"]["limit"]
            .get("output")
            .is_none());
    }

    #[test]
    fn unknown_extras_ignored() {
        let (_td, paths) = setup();
        init_live(&paths);
        let mut p = provider(Some("gpt-4o"));
        p.extras.insert("nope".into(), "xyz".into());
        p.extras.insert("model_name".into(), "Pretty".into());
        OpenCodeAdapter.validate(&p).unwrap();
        OpenCodeAdapter.apply(&paths, &p).unwrap();
        let doc = read_value(&paths.opencode_dir.join("opencode.json"));
        assert!(doc.get("nope").is_none());
        assert_eq!(
            doc["provider"]["PackyCode"]["models"]["gpt-4o"]["name"],
            "gpt-4o"
        );
        assert_ne!(
            doc["provider"]["PackyCode"]["models"]["gpt-4o"]["name"],
            "Pretty"
        );
    }

    #[test]
    fn validate_rejects_empty_and_bad_url() {
        let mut p = provider(Some("gpt-4o"));
        p.name.clear();
        assert!(OpenCodeAdapter.validate(&p).is_err());
        p = provider(Some("gpt-4o"));
        p.base_url = "ftp://x".into();
        assert!(OpenCodeAdapter.validate(&p).is_err());
        p = provider(Some("gpt-4o"));
        p.api_key.clear();
        assert!(OpenCodeAdapter.validate(&p).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn existing_live_perms_preserved() {
        use std::os::unix::fs::PermissionsExt;
        let (_td, paths) = setup();
        init_live(&paths);
        let live = paths.opencode_dir.join("opencode.json");
        fs::write(&live, b"{}\n").unwrap();
        fs::set_permissions(&live, fs::Permissions::from_mode(0o644)).unwrap();
        OpenCodeAdapter
            .apply(&paths, &provider(Some("gpt-4o")))
            .unwrap();
        let mode = fs::metadata(&live).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
    }

    #[test]
    fn isolation_apply_does_not_touch_host() {
        let (_td, paths) = setup();
        init_live(&paths);
        OpenCodeAdapter
            .apply(&paths, &provider(Some("gpt-4o")))
            .unwrap();
        crate::fsutil::panic_if_host_config_path(&paths.opencode_dir.join("opencode.json"));
        let host = dirs::home_dir()
            .expect("home")
            .join(".config")
            .join("opencode");
        assert_ne!(paths.opencode_dir, host);
    }

    #[test]
    fn registry_opencode_fields() {
        let a = get(AppId::OpenCode).unwrap();
        assert_eq!(a.display_name(), "OpenCode");
        assert_eq!(a.fields().len(), 5);
        let protocol = a.fields().iter().find(|f| f.key == "protocol").unwrap();
        assert!(matches!(protocol.kind, FieldKind::Select(_)));
        assert!(!protocol.required);
        assert!(a.fields().iter().any(|f| f.key == "model" && f.required));
        assert!(registry().iter().any(|x| x.id() == AppId::OpenCode));
        let extras = parse_extras(a, &["protocol=anthropic".into()]).unwrap();
        assert_eq!(
            extras.get("protocol").map(String::as_str),
            Some("anthropic")
        );
        let err = parse_extras(a, &["protocol=google".into()]).unwrap_err();
        assert!(err.to_string().contains("invalid value"));
        let err = parse_extras(a, &["npm=@custom/sdk".into()]).unwrap_err();
        assert!(err.to_string().contains("unknown extra field"));
    }
}
