use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::fsutil;
use crate::paths::Paths;

pub const STORE_VERSION: u32 = 1;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum AppId {
    Claude,
    Codex,
    OpenCode,
    Pi,
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AppId::Claude => "claude",
            AppId::Codex => "codex",
            AppId::OpenCode => "opencode",
            AppId::Pi => "pi",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Store {
    pub version: u32,
    #[serde(default)]
    pub current: BTreeMap<AppId, String>,
    /// The provider slot key currently injected into each app's live config
    /// (the display name it was written under). Cleared/replaced on switch.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub slot_keys: BTreeMap<AppId, String>,
    pub providers: IndexMap<String, Provider>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ModelEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// Claude-only: the Anthropic model ID this row proxies. When set to a
    /// string Claude Code recognises (e.g. `claude-opus-4-8`; the known-id
    /// table deliberately lists only undated aliases), the adapter writes
    /// `ANTHROPIC_DEFAULT_*_MODEL = <target>` for any slot the row owns,
    /// and emits a `modelOverrides[<target>] = <id>` entry that translates
    /// the Anthropic ID to the row's actual id at request time. When `None`
    /// or not in the known-id table, the row stays an unknown proxy id and
    /// Claude Code prints its "unrecognised model" warning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_model_id: Option<String>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

pub(crate) fn is_empty_snippet(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => true,
        serde_json::Value::Object(m) if m.is_empty() => true,
        _ => false,
    }
}

pub(crate) fn skip_snippet(v: &Option<serde_json::Value>) -> bool {
    v.as_ref().is_none_or(is_empty_snippet)
}

pub(crate) fn normalize_snippet(v: Option<serde_json::Value>) -> Option<serde_json::Value> {
    if skip_snippet(&v) {
        None
    } else {
        v
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub app: AppId,
    pub base_url: String,
    pub api_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub catalog: Vec<ModelEntry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub slots: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "skip_snippet")]
    pub snippet: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub apply_snippet: bool,
    /// Built-in per-app row that restores the CLI's native subscription
    /// (Claude Official / OpenAI Official). Seeded on load, not deletable.
    #[serde(default, skip_serializing_if = "is_false")]
    pub official: bool,
}

impl Provider {
    /// The key this provider occupies in third-party live configs: its
    /// display name (Codex `[model_providers."…"]`, OpenCode `provider."…"`,
    /// Pi `providers."…"`). Control characters are flattened; a blank name
    /// falls back to the neutral [`crate::name::DEFAULT_SLOT_KEY`].
    pub fn slot_key(&self) -> String {
        let t = self.name.trim();
        if t.is_empty() {
            return crate::name::DEFAULT_SLOT_KEY.to_string();
        }
        t.chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    }

    pub fn blank(app: AppId) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            app,
            base_url: String::new(),
            api_key: String::new(),
            model: None,
            extras: BTreeMap::new(),
            catalog: Vec::new(),
            slots: BTreeMap::new(),
            snippet: None,
            apply_snippet: false,
            official: false,
        }
    }
}

/// Built-in provider that hands the CLI back to its native subscription:
/// empty base_url/api_key on purpose, adapters strip their owned fields.
pub fn official_provider(app: AppId) -> Option<Provider> {
    let mut p = Provider::blank(app);
    match app {
        AppId::Claude => {
            p.id = "claude-official".into();
            p.name = "Claude Official".into();
        }
        AppId::Codex => {
            p.id = "codex-official".into();
            p.name = "OpenAI Official".into();
        }
        // No native-login path for these apps.
        AppId::OpenCode | AppId::Pi => return None,
    }
    p.official = true;
    Some(p)
}

#[derive(Debug, Deserialize)]
struct DraftStore {
    #[serde(default)]
    providers: IndexMap<String, DraftProvider>,
    #[serde(default)]
    current: String,
}

#[derive(Debug, Deserialize)]
struct DraftProvider {
    #[serde(default)]
    id: String,
    name: String,
    app: DraftApp,
    base_url: String,
    api_key: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    wire_api: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DraftApp {
    Claude,
    Codex,
    OpenCode,
    Pi,
    Gemini,
}

impl DraftApp {
    fn to_app_id(self) -> Option<AppId> {
        match self {
            DraftApp::Claude => Some(AppId::Claude),
            DraftApp::Codex => Some(AppId::Codex),
            DraftApp::OpenCode => Some(AppId::OpenCode),
            DraftApp::Pi => Some(AppId::Pi),
            DraftApp::Gemini => None,
        }
    }
}

impl Store {
    pub fn empty() -> Self {
        Self {
            version: STORE_VERSION,
            current: BTreeMap::new(),
            slot_keys: BTreeMap::new(),
            providers: IndexMap::new(),
        }
    }

    pub fn load(paths: &Paths) -> Result<Self> {
        let mut store = Self::load_inner(paths)?;
        store.ensure_official_providers();
        Ok(store)
    }

    /// Idempotently seed the built-in official rows (Claude/Codex) and pin
    /// them to the top of the list.
    pub fn ensure_official_providers(&mut self) {
        let mut offset = 0;
        for app in crate::settings::all_apps() {
            let Some(seed) = official_provider(app) else {
                continue;
            };
            let found = self.providers.get_full(&seed.id).map(|(idx, _, _)| idx);
            match found {
                Some(idx) => {
                    if let Some((_, existing)) = self.providers.get_index_mut(idx) {
                        existing.official = true;
                    }
                    if idx != offset {
                        self.providers.move_index(idx, offset);
                    }
                }
                None => {
                    self.providers.shift_insert(offset, seed.id.clone(), seed);
                }
            }
            offset += 1;
        }
    }

    fn load_inner(paths: &Paths) -> Result<Self> {
        let store_path = paths.store_file();
        let draft_path = paths.draft_file();

        if store_path.exists() {
            let (mut store, dropped_gemini) = load_store_json(&store_path)?;
            if store.version < STORE_VERSION {
                log::debug!(
                    "store.load migrate version={} -> {}",
                    store.version,
                    STORE_VERSION
                );
                return migrate_older(store, paths);
            }
            let seeded = seed_claude_catalog(&mut store);
            if dropped_gemini || seeded {
                // also covers copying legacy store.snippets onto providers
                store.save(paths)?;
            }
            log::debug!("store.load path={}", store_path.display());
            return Ok(store);
        }

        if draft_path.exists() {
            log::debug!("store.load migrate draft={}", draft_path.display());
            return migrate_draft(paths, &draft_path);
        }

        log::debug!("store.load empty (no store.json)");
        Ok(Self::empty())
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        fsutil::ensure_dir_0700(&paths.aimux_dir)?;
        let path = paths.store_file();
        let mut data = serde_json::to_string_pretty(self).context("serialize store.json")?;
        if !data.ends_with('\n') {
            data.push('\n');
        }
        fsutil::atomic_write(&path, data.as_bytes())?;
        log::debug!("store.save path={}", path.display());
        Ok(())
    }

    /// Load a store snapshot from an arbitrary JSON file (backup restore).
    /// Does not migrate drafts or rewrite the source file.
    pub(crate) fn load_from_file(path: &Path) -> Result<Self> {
        let (store, _) = load_store_json(path)?;
        Ok(store)
    }

    /// Parse store bytes (remote pull). Does not write disk.
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let path = Path::new("store.json");
        let data = std::str::from_utf8(bytes).map_err(|e| {
            Error::io(
                path,
                std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            )
        })?;
        let (store, _) = parse_store_json(path, data)?;
        Ok(store)
    }
}

fn load_store_json(path: &Path) -> Result<(Store, bool)> {
    let data = fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    parse_store_json(path, &data)
}

fn parse_store_json(path: &Path, data: &str) -> Result<(Store, bool)> {
    let mut value: serde_json::Value =
        serde_json::from_str(data).map_err(|e| Error::json(path, e))?;
    if let Some(found) = value.get("version").and_then(serde_json::Value::as_u64) {
        if found > u64::from(STORE_VERSION) {
            return Err(Error::UnsupportedStoreVersion {
                found: u32::try_from(found).unwrap_or(u32::MAX),
                supported: STORE_VERSION,
            }
            .into());
        }
    }
    let dropped_gemini = drop_gemini_from_store_value(&mut value);
    let legacy_snippets = value.as_object_mut().and_then(|o| o.remove("snippets"));
    let mut store: Store = serde_json::from_value(value).map_err(|e| Error::json(path, e))?;
    let migrated_snippets = migrate_legacy_snippets(&mut store, legacy_snippets);
    Ok((store, dropped_gemini || migrated_snippets))
}

fn migrate_legacy_snippets(store: &mut Store, snippets: Option<serde_json::Value>) -> bool {
    let Some(snippets) = snippets else {
        return false;
    };
    let Some(map) = snippets.as_object() else {
        return true;
    };
    if map.is_empty() {
        return true;
    }
    for p in store.providers.values_mut() {
        if !skip_snippet(&p.snippet) {
            continue;
        }
        let key = p.app.to_string();
        if let Some(s) = map.get(&key) {
            p.snippet = normalize_snippet(Some(s.clone()));
        }
    }
    true
}

fn warn_drop_gemini(id: &str) {
    log::warn!("store.load dropping provider {id}: app=gemini is no longer supported");
}

fn drop_gemini_from_store_value(value: &mut serde_json::Value) -> bool {
    let Some(obj) = value.as_object_mut() else {
        return false;
    };
    let mut dropped = false;
    let mut dropped_ids: Vec<String> = Vec::new();
    if let Some(providers) = obj.get_mut("providers").and_then(|v| v.as_object_mut()) {
        let ids: Vec<String> = providers.keys().cloned().collect();
        for id in ids {
            let is_gemini = providers
                .get(&id)
                .and_then(|p| p.get("app"))
                .and_then(|a| a.as_str())
                == Some("gemini");
            if is_gemini {
                warn_drop_gemini(&id);
                providers.remove(&id);
                dropped_ids.push(id);
                dropped = true;
            }
        }
    }
    if let Some(current) = obj.get_mut("current").and_then(|v| v.as_object_mut()) {
        if current.remove("gemini").is_some() {
            log::warn!("store.load dropping current.gemini: app=gemini is no longer supported");
            dropped = true;
        }
        current.retain(|_, v| match v.as_str() {
            Some(id) => !dropped_ids.iter().any(|d| d == id),
            None => true,
        });
    }
    dropped
}

/// Idempotently seed the catalog of every Claude provider that doesn't
/// have one yet, so the new catalog-style editor has rows to display.
///
/// Rule (per Q6 of `docs/claude-catalog-model-window.md`):
/// 1. If `provider.model` is `Some(id)`, seed one row with that id.
/// 2. For each non-empty `provider.slots` value not already covered by (1)
///    or the existing catalog, seed one row with that id.
/// 3. If (1) and (2) produced nothing, the editor renders an in-memory
///    "add a model" placeholder; the SSOT stays empty.
///
/// Returns `true` if any row was added (caller persists).
fn seed_claude_catalog(store: &mut Store) -> bool {
    // The 5 keys here must mirror `crate::adapter::models::CLAUDE_SLOTS`.
    // Duplicated to avoid a `store -> adapter` dependency cycle.
    const CLAUDE_SLOT_KEYS: &[&str] = &["haiku", "sonnet", "opus", "fable", "subagent"];

    let mut mutated = false;
    for p in store.providers.values_mut() {
        if p.app != AppId::Claude {
            continue;
        }
        if p.official {
            // Officials strip their catalog in apply; no point seeding.
            continue;
        }
        if !p.catalog.is_empty() {
            // Already migrated; idempotent.
            continue;
        }

        // Step 1: default model.
        let mut seeded_ids: Vec<String> = Vec::new();
        if let Some(id) = p.model.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            p.catalog.push(ModelEntry {
                id: id.to_string(),
                label: None,
                context_window: None,
                max_tokens: None,
                target_model_id: None,
            });
            seeded_ids.push(id.to_string());
            mutated = true;
        }

        // Step 2: each non-empty slot value that is not already represented.
        for key in CLAUDE_SLOT_KEYS {
            if *key == "subagent" {
                // `subagent` is the agent-role slot; not a model a user
                // typically edits. Skip from seeding to keep the grid focused.
                continue;
            }
            let Some(id) = p
                .slots
                .get(*key)
                .map(String::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            if seeded_ids.iter().any(|s| s == id) {
                continue;
            }
            p.catalog.push(ModelEntry {
                id: id.to_string(),
                label: None,
                context_window: None,
                max_tokens: None,
                target_model_id: None,
            });
            seeded_ids.push(id.to_string());
            mutated = true;
        }
    }
    mutated
}

fn migrate_older(mut store: Store, paths: &Paths) -> Result<Store> {
    store.version = STORE_VERSION;
    store.save(paths)?;
    Ok(store)
}

fn migrate_draft(paths: &Paths, draft_path: &Path) -> Result<Store> {
    let data = fs::read_to_string(draft_path).map_err(|e| Error::io(draft_path, e))?;
    let draft: DraftStore = serde_json::from_str(&data).map_err(|e| Error::json(draft_path, e))?;

    let mut providers = IndexMap::new();
    for (key, dp) in draft.providers {
        let id = if dp.id.is_empty() { key } else { dp.id };
        let Some(app) = dp.app.to_app_id() else {
            warn_drop_gemini(&id);
            continue;
        };
        let mut extras = BTreeMap::new();
        if let Some(wire_api) = dp.wire_api {
            if !wire_api.is_empty() {
                extras.insert("wire_api".to_string(), wire_api);
            }
        }
        providers.insert(
            id.clone(),
            Provider {
                id,
                name: dp.name,
                app,
                base_url: dp.base_url,
                api_key: dp.api_key,
                model: dp.model.filter(|m| !m.is_empty()),
                extras,
                ..Provider::blank(app)
            },
        );
    }

    let mut current = BTreeMap::new();
    if !draft.current.is_empty() {
        if let Some(p) = providers.get(&draft.current) {
            current.insert(p.app, p.id.clone());
        }
    }

    let store = Store {
        version: STORE_VERSION,
        current,
        slot_keys: BTreeMap::new(),
        providers,
    };
    store.save(paths)?;

    let bak = paths.aimux_dir.join("providers.json.bak");
    fsutil::rename_replace(draft_path, &bak)?;
    fsutil::chmod_file_0600(&bak)?;
    log::debug!("store.load migrated draft -> {}", bak.display());
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn setup() -> (tempfile::TempDir, Paths) {
        let td = tempfile::tempdir().expect("tempdir");
        let paths = Paths::for_test(td.path());
        (td, paths)
    }

    #[cfg(unix)]
    fn unix_mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn assert_not_host(path: &Path) {
        crate::fsutil::panic_if_host_config_path(path);
    }

    #[test]
    fn catalog_slots_and_snippets_round_trip() {
        let (_td, paths) = setup();
        let mut store = Store::empty();
        let mut p = Provider::blank(AppId::Claude);
        p.id = "packy".into();
        p.name = "Packy".into();
        p.base_url = "https://api.example.com".into();
        p.api_key = "sk-test-key-abcd".into();
        p.model = Some("sonnet".into());
        p.slots.insert("haiku".into(), "haiku-id".into());
        p.snippet = Some(serde_json::json!({"includeCoAuthoredBy": false}));
        p.apply_snippet = true;
        store.providers.insert("packy".into(), p);
        store.save(&paths).unwrap();
        let loaded = Store::load(&paths).unwrap();
        assert_eq!(loaded.providers["packy"].slots["haiku"], "haiku-id");
        assert!(loaded.providers["packy"].apply_snippet);
        assert_eq!(
            loaded.providers["packy"].snippet.as_ref().unwrap()["includeCoAuthoredBy"],
            false
        );
        let text = fs::read_to_string(paths.store_file()).unwrap();
        // `catalog` is now seeded from provider.model on first load; the slot
        // haiku-id dedupes against model so no second row is added.
        assert!(text.contains("\"catalog\""));
        assert!(text.contains("\"id\": \"sonnet\""));
        assert!(!text.contains("\"snippets\""));
    }

    #[test]
    fn legacy_app_snippets_copy_onto_providers() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        fs::write(
            paths.store_file(),
            r#"{
              "version": 1,
              "current": {},
              "providers": {
                "packy": {
                  "id": "packy",
                  "name": "Packy",
                  "app": "claude",
                  "base_url": "https://api.example.com",
                  "api_key": "sk-test-key-abcd"
                }
              },
              "snippets": {
                "claude": { "includeCoAuthoredBy": false }
              }
            }"#,
        )
        .unwrap();
        let loaded = Store::load(&paths).unwrap();
        assert_eq!(
            loaded.providers["packy"].snippet.as_ref().unwrap()["includeCoAuthoredBy"],
            false
        );
        let text = fs::read_to_string(paths.store_file()).unwrap();
        assert!(!text.contains("\"snippets\""));
        assert!(text.contains("includeCoAuthoredBy"));
    }

    #[test]
    fn empty_store_when_nothing_exists_does_not_write() {
        let (td, paths) = setup();
        assert_not_host(&paths.store_file());
        let store = Store::load(&paths).unwrap();
        // Nothing persisted; the seeded in-memory view carries only officials.
        assert_eq!(store, {
            let mut s = Store::empty();
            s.ensure_official_providers();
            s
        });
        assert!(!paths.store_file().exists());
        assert!(!paths.aimux_dir.exists());
        drop(td);
    }

    #[test]
    fn official_rows_are_pinned_to_the_top() {
        let mut store = Store::empty();
        let mut user = Provider::blank(AppId::Claude);
        user.id = "user-claude".into();
        user.name = "User Claude".into();
        store.providers.insert(user.id.clone(), user.clone());
        // A previous version persisted the officials mid-list.
        let stale = official_provider(AppId::Codex).unwrap();
        store.providers.insert(stale.id.clone(), stale);

        store.ensure_official_providers();
        let ids: Vec<&str> = store.providers.keys().map(String::as_str).collect();
        assert_eq!(
            ids,
            vec!["claude-official", "codex-official", "user-claude"]
        );
        assert!(store.providers["codex-official"].official); // re-pinned flag

        // Idempotent: a second pass keeps the same order.
        store.ensure_official_providers();
        let ids: Vec<&str> = store.providers.keys().map(String::as_str).collect();
        assert_eq!(
            ids,
            vec!["claude-official", "codex-official", "user-claude"]
        );
    }

    #[test]
    fn future_version_is_rejected() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        fs::write(
            paths.store_file(),
            r#"{"version":99,"current":{},"providers":{}}"#,
        )
        .unwrap();
        let err = Store::load(&paths).unwrap_err();
        let e = err
            .downcast_ref::<Error>()
            .unwrap_or_else(|| panic!("expected Error, got {err:?}"));
        match e {
            Error::UnsupportedStoreVersion {
                found: 99,
                supported: STORE_VERSION,
            } => {}
            other => panic!("unexpected error: {other:?}"),
        }
        let body = fs::read_to_string(paths.store_file()).unwrap();
        assert!(
            body.contains("\"version\":99"),
            "must not rewrite a future store"
        );
    }

    #[test]
    fn future_version_incompatible_schema_is_rejected() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        fs::write(
            paths.store_file(),
            r#"{"version":99,"current":"packy","providers":[]}"#,
        )
        .unwrap();
        let err = Store::load(&paths).unwrap_err();
        let e = err
            .downcast_ref::<Error>()
            .unwrap_or_else(|| panic!("expected Error, got {err:?}"));
        match e {
            Error::UnsupportedStoreVersion {
                found: 99,
                supported: STORE_VERSION,
            } => {}
            other => panic!("unexpected error: {other:?}"),
        }
        let body = fs::read_to_string(paths.store_file()).unwrap();
        assert_eq!(body, r#"{"version":99,"current":"packy","providers":[]}"#);
    }

    #[test]
    fn roundtrip_save_load() {
        let (_td, paths) = setup();
        let mut store = Store::empty();
        store.providers.insert(
            "packy".into(),
            Provider {
                id: "packy".into(),
                name: "PackyCode".into(),
                app: AppId::Claude,
                base_url: "https://api.example.com".into(),
                api_key: "sk-test".into(),
                model: Some("claude-sonnet".into()),
                extras: BTreeMap::from([("api_key_field".into(), "auth_token".into())]),
                ..Provider::blank(AppId::Claude)
            },
        );
        store.current.insert(AppId::Claude, "packy".into());
        store.save(&paths).unwrap();
        let loaded = Store::load(&paths).unwrap();
        // Load seeds the built-in official rows on both sides, and migrates
        // empty claude catalogs from provider.model.
        store.ensure_official_providers();
        let _ = seed_claude_catalog(&mut store);
        assert_eq!(loaded, store);
        assert!(
            !serde_json::to_string(&loaded)
                .unwrap()
                .contains("\"opencode\""),
            "absent apps must be omitted from current"
        );
    }

    #[test]
    fn draft_migration_promotes_wire_api_and_current() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        let draft = r#"{
            "providers": {
                "packy": {
                    "id": "packy",
                    "name": "PackyCode",
                    "app": "codex",
                    "base_url": "https://api.example.com/v1",
                    "api_key": "sk-test",
                    "model": "gpt-5",
                    "wire_api": "responses"
                }
            },
            "current": "packy"
        }"#;
        fs::write(paths.draft_file(), draft).unwrap();

        let store = Store::load(&paths).unwrap();
        assert_eq!(store.version, STORE_VERSION);
        let p = store.providers.get("packy").expect("packy");
        assert_eq!(p.app, AppId::Codex);
        assert_eq!(
            p.extras.get("wire_api").map(String::as_str),
            Some("responses")
        );
        assert_eq!(
            store.current.get(&AppId::Codex).map(String::as_str),
            Some("packy")
        );
        assert!(!store.current.contains_key(&AppId::Claude));

        assert!(paths.store_file().exists());
        assert!(!paths.draft_file().exists());
        assert!(paths.aimux_dir.join("providers.json.bak").exists());

        #[cfg(unix)]
        {
            assert_eq!(unix_mode(&paths.store_file()), 0o600);
            assert_eq!(
                unix_mode(&paths.aimux_dir.join("providers.json.bak")),
                0o600
            );
            assert_eq!(unix_mode(&paths.aimux_dir), 0o700);
        }
    }

    #[test]
    fn draft_gemini_records_are_dropped_not_mapped() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        fs::write(
            paths.draft_file(),
            r#"{
                "providers": {
                    "g": {
                        "id": "g",
                        "name": "Gemini Proxy",
                        "app": "gemini",
                        "base_url": "https://example.com",
                        "api_key": "k"
                    },
                    "c": {
                        "id": "c",
                        "name": "Claude",
                        "app": "claude",
                        "base_url": "https://example.com",
                        "api_key": "k"
                    }
                },
                "current": "g"
            }"#,
        )
        .unwrap();
        let store = Store::load(&paths).unwrap();
        assert!(!store.providers.contains_key("g"));
        assert!(store.providers.contains_key("c"));
        assert_eq!(store.providers["c"].app, AppId::Claude);
        assert!(store.current.is_empty());
        let body = fs::read_to_string(paths.store_file()).unwrap();
        assert!(
            !body.contains("gemini") && !body.contains("opencode"),
            "dropped gemini must not be rewritten as opencode: {body}"
        );
    }

    #[test]
    fn store_json_gemini_providers_and_current_are_dropped() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        fs::write(
            paths.store_file(),
            r#"{
                "version": 1,
                "current": {
                    "gemini": "g",
                    "claude": "c"
                },
                "providers": {
                    "g": {
                        "id": "g",
                        "name": "Gemini Proxy",
                        "app": "gemini",
                        "base_url": "https://example.com",
                        "api_key": "k"
                    },
                    "c": {
                        "id": "c",
                        "name": "Claude",
                        "app": "claude",
                        "base_url": "https://example.com",
                        "api_key": "k"
                    }
                }
            }"#,
        )
        .unwrap();
        let store = Store::load(&paths).unwrap();
        assert!(!store.providers.contains_key("g"));
        assert!(store.providers.contains_key("c"));
        assert_eq!(
            store.current.get(&AppId::Claude).map(String::as_str),
            Some("c")
        );
        assert!(!store.current.keys().any(|a| a.to_string() == "gemini"));
        let body = fs::read_to_string(paths.store_file()).unwrap();
        assert!(
            !body.contains("gemini"),
            "one-time drop must rewrite store.json without gemini: {body}"
        );
        let loaded = Store::load(&paths).unwrap();
        assert_eq!(loaded, store);
    }

    #[test]
    fn draft_missing_current_id_yields_empty_current() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        fs::write(
            paths.draft_file(),
            r#"{
                "providers": {
                    "a": {
                        "id": "a",
                        "name": "A",
                        "app": "claude",
                        "base_url": "https://example.com",
                        "api_key": "k"
                    }
                },
                "current": "missing"
            }"#,
        )
        .unwrap();
        let store = Store::load(&paths).unwrap();
        assert!(store.current.is_empty());
        assert!(store.providers.contains_key("a"));
    }

    #[test]
    fn store_json_wins_over_draft() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        fs::write(
            paths.store_file(),
            r#"{"version":1,"current":{},"providers":{}}"#,
        )
        .unwrap();
        fs::write(
            paths.draft_file(),
            r#"{"providers":{"a":{"id":"a","name":"A","app":"claude","base_url":"u","api_key":"k"}},"current":"a"}"#,
        )
        .unwrap();
        let store = Store::load(&paths).unwrap();
        // No user rows; only the seeded built-in officials are present.
        assert!(store.providers.values().all(|p| p.official));
        assert!(paths.draft_file().exists());
    }

    #[test]
    fn seed_skips_official_providers() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        fs::write(
            paths.store_file(),
            r#"{"version":1,"current":{},"providers":{"claude-official":{"id":"claude-official","name":"Claude Official","app":"claude","base_url":"","api_key":"","official":true}}}"#,
        )
        .unwrap();
        let store = Store::load(&paths).unwrap();
        assert!(store.providers["claude-official"].catalog.is_empty());
    }

    #[test]
    fn seed_dedupes_default_model_against_slots() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        fs::write(
            paths.store_file(),
            r#"{"version":1,"current":{},"providers":{"p":{"id":"p","name":"P","app":"claude","base_url":"u","api_key":"k","model":"shared","slots":{"haiku":"shared","sonnet":"other"}}}}"#,
        )
        .unwrap();
        let store = Store::load(&paths).unwrap();
        let cat = &store.providers["p"].catalog;
        let ids: Vec<&str> = cat.iter().map(|m| m.id.as_str()).collect();
        // default "shared" + slot "sonnet" -> "other"; haiku skipped (same as default).
        assert_eq!(ids, vec!["shared", "other"]);
    }

    #[test]
    fn seed_runs_only_when_catalog_empty() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        fs::write(
            paths.store_file(),
            r#"{"version":1,"current":{},"providers":{"p":{"id":"p","name":"P","app":"claude","base_url":"u","api_key":"k","model":"ignored","catalog":[{"id":"existing","context_window":12345}]}}}"#,
        )
        .unwrap();
        let store = Store::load(&paths).unwrap();
        let cat = &store.providers["p"].catalog;
        // Already populated -> not overwritten.
        assert_eq!(cat.len(), 1);
        assert_eq!(cat[0].id, "existing");
        assert_eq!(cat[0].context_window, Some(12345));
    }

    #[test]
    fn seed_skips_subagent_slot() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        fs::write(
            paths.store_file(),
            r#"{"version":1,"current":{},"providers":{"p":{"id":"p","name":"P","app":"claude","base_url":"u","api_key":"k","slots":{"subagent":"agent-only","haiku":"h"}}}}"#,
        )
        .unwrap();
        let store = Store::load(&paths).unwrap();
        let cat = &store.providers["p"].catalog;
        let ids: Vec<&str> = cat.iter().map(|m| m.id.as_str()).collect();
        // subagent skipped, haiku only.
        assert_eq!(ids, vec!["h"]);
    }

    #[test]
    fn seed_leaves_non_claude_apps_alone() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        fs::write(
            paths.store_file(),
            r#"{"version":1,"current":{},"providers":{"p":{"id":"p","name":"P","app":"codex","base_url":"u","api_key":"k","model":"gpt-4o"}}}"#,
        )
        .unwrap();
        let store = Store::load(&paths).unwrap();
        assert!(store.providers["p"].catalog.is_empty());
    }

    #[test]
    fn seed_is_idempotent() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        fs::write(
            paths.store_file(),
            r#"{"version":1,"current":{},"providers":{"p":{"id":"p","name":"P","app":"claude","base_url":"u","api_key":"k","model":"m","slots":{"haiku":"h"}}}}"#,
        )
        .unwrap();
        let first = Store::load(&paths).unwrap();
        let after = Store::load(&paths).unwrap();
        assert_eq!(first.providers["p"].catalog, after.providers["p"].catalog);
        assert_eq!(after.providers["p"].catalog.len(), 2);
    }

    #[test]
    fn seed_persists_rewritten_store() {
        let (_td, paths) = setup();
        fsutil::ensure_dir_0700(&paths.aimux_dir).unwrap();
        fs::write(
            paths.store_file(),
            r#"{"version":1,"current":{},"providers":{"p":{"id":"p","name":"P","app":"claude","base_url":"u","api_key":"k","model":"m"}}}"#,
        )
        .unwrap();
        Store::load(&paths).unwrap();
        // Reload from disk to confirm the seeded row was actually written out.
        let body = fs::read_to_string(paths.store_file()).unwrap();
        assert!(body.contains("\"catalog\""));
        assert!(body.contains("\"id\": \"m\""));
    }

    #[test]
    fn model_entry_target_model_id_default_skipped() {
        let entry = ModelEntry::default();
        assert!(serde_json::to_string(&entry)
            .unwrap()
            .contains("\"id\":\"\""));
        assert!(!serde_json::to_string(&entry)
            .unwrap()
            .contains("target_model_id"));
    }

    #[test]
    fn model_entry_target_model_id_round_trip() {
        let entry = ModelEntry {
            id: "m".into(),
            target_model_id: Some("claude-sonnet-4-6-20251001".into()),
            ..ModelEntry::default()
        };
        let text = serde_json::to_string(&entry).unwrap();
        assert!(text.contains("\"target_model_id\":\"claude-sonnet-4-6-20251001\""));
        let back: ModelEntry = serde_json::from_str(&text).unwrap();
        assert_eq!(
            back.target_model_id.as_deref(),
            Some("claude-sonnet-4-6-20251001")
        );
    }

    #[test]
    fn isolation_store_save_does_not_touch_host() {
        let (_td, paths) = setup();
        let host = dirs::home_dir().expect("home");
        assert_ne!(paths.aimux_dir, host.join(crate::name::DOT_DIR));
        Store::empty().save(&paths).unwrap();
        crate::fsutil::panic_if_host_config_path(&paths.store_file());
        assert!(
            !host.join(crate::name::DOT_DIR).join("store.json").exists()
                || paths.aimux_dir != host.join(crate::name::DOT_DIR)
        );
    }
}
