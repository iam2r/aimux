//! TUI settings (`settings.json`). Not part of store.json.

use std::collections::BTreeSet;
use std::fs;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::adapter::{self, AppId};
use crate::fsutil;
use crate::i18n;
use crate::paths::Paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppsMode {
    #[default]
    Auto,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(default)]
    pub apps_mode: AppsMode,
    #[serde(default)]
    pub visible: Vec<AppId>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lang: None,
            apps_mode: AppsMode::Auto,
            visible: Vec::new(),
        }
    }
}

impl Settings {
    pub fn load(paths: &Paths) -> Result<Self> {
        let path = paths.settings_file();
        if !path.is_file() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(&path).map_err(|e| crate::error::Error::io(&path, e))?;
        let settings: Self =
            serde_json::from_str(&data).map_err(|e| crate::error::Error::json(&path, e))?;
        Ok(settings)
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        fsutil::ensure_dir_0700(&paths.aimux_dir)?;
        let path = paths.settings_file();
        let mut body = serde_json::to_string_pretty(self).context("serialize settings.json")?;
        if !body.ends_with('\n') {
            body.push('\n');
        }
        fsutil::atomic_write(&path, body.as_bytes())?;
        Ok(())
    }

    pub fn lang_enum(&self) -> Option<i18n::Lang> {
        self.lang.as_deref().and_then(i18n::parse_tag)
    }

    pub fn set_lang(&mut self, lang: i18n::Lang) {
        self.lang = Some(match lang {
            i18n::Lang::En => "en".into(),
            i18n::Lang::Zh => "zh".into(),
        });
        i18n::set(lang);
    }

    pub fn cycle_lang(&mut self) {
        let next = match self.lang_enum().unwrap_or_else(i18n::lang) {
            i18n::Lang::En => i18n::Lang::Zh,
            i18n::Lang::Zh => i18n::Lang::En,
        };
        self.set_lang(next);
    }

    pub fn cycle_apps_mode(&mut self, detected: &[AppId]) {
        match self.apps_mode {
            AppsMode::Auto => {
                self.apps_mode = AppsMode::Manual;
                if self.visible.is_empty() {
                    self.visible = if detected.is_empty() {
                        all_apps()
                    } else {
                        detected.to_vec()
                    };
                }
            }
            AppsMode::Manual => {
                self.apps_mode = AppsMode::Auto;
            }
        }
    }

    pub fn toggle_visible(&mut self, app: AppId) {
        if self.apps_mode != AppsMode::Manual {
            return;
        }
        if let Some(i) = self.visible.iter().position(|a| *a == app) {
            if self.visible.len() == 1 {
                return;
            }
            self.visible.remove(i);
        } else {
            self.visible.push(app);
        }
    }

    pub fn is_manually_visible(&self, app: AppId) -> bool {
        self.visible.contains(&app)
    }
}

pub fn all_apps() -> Vec<AppId> {
    adapter::registry().iter().map(|a| a.id()).collect()
}

/// Same binaries cc-switch probes with `which`.
pub fn binary_names(app: AppId) -> &'static [&'static str] {
    match app {
        AppId::Claude => &["claude"],
        AppId::Codex => &["codex"],
        AppId::OpenCode => &["opencode"],
        AppId::Pi => &["pi"],
    }
}

pub fn on_path(bin: &str) -> bool {
    which::which(bin).is_ok()
}

pub fn tool_installed(app: AppId) -> bool {
    binary_names(app).iter().any(|bin| on_path(bin))
}

/// Auto detection = the CLI binary is on PATH (same probe as cc-switch).
/// Leftover config dirs or old store rows do NOT count as an agent.
pub fn detected(app: AppId) -> bool {
    tool_installed(app)
}

pub fn detected_apps() -> Vec<AppId> {
    all_apps()
        .into_iter()
        .filter(|app| detected(*app))
        .collect()
}

pub fn visible_apps(settings: &Settings, detected: &[AppId]) -> Vec<AppId> {
    match settings.apps_mode {
        AppsMode::Auto => {
            if detected.is_empty() {
                all_apps()
            } else {
                detected.to_vec()
            }
        }
        AppsMode::Manual => {
            let set: BTreeSet<AppId> = settings.visible.iter().copied().collect();
            let vis: Vec<AppId> = all_apps().into_iter().filter(|a| set.contains(a)).collect();
            if vis.is_empty() {
                all_apps()
            } else {
                vis
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Paths) {
        let td = tempfile::tempdir().unwrap();
        let paths = Paths::for_test(td.path());
        (td, paths)
    }

    #[test]
    fn default_is_auto() {
        let s = Settings::default();
        assert_eq!(s.apps_mode, AppsMode::Auto);
        assert!(s.visible.is_empty());
        assert_eq!(visible_apps(&s, &[]), all_apps());
        assert_eq!(visible_apps(&s, &[AppId::Claude]), vec![AppId::Claude]);
    }

    #[test]
    fn roundtrip_and_manual_toggle() {
        let (_td, paths) = setup();
        let mut s = Settings::default();
        s.cycle_apps_mode(&[AppId::Claude, AppId::Codex]);
        assert_eq!(s.apps_mode, AppsMode::Manual);
        assert_eq!(s.visible, vec![AppId::Claude, AppId::Codex]);
        s.toggle_visible(AppId::Codex);
        assert_eq!(s.visible, vec![AppId::Claude]);
        s.toggle_visible(AppId::Claude);
        assert_eq!(s.visible, vec![AppId::Claude]);
        s.save(&paths).unwrap();
        let loaded = Settings::load(&paths).unwrap();
        assert_eq!(loaded.apps_mode, AppsMode::Manual);
        assert_eq!(loaded.visible, vec![AppId::Claude]);
    }

    #[test]
    fn detection_is_path_only() {
        // Leftover config dirs and store rows must NOT resurrect an app whose
        // CLI binary is gone; only the `which` probe decides.
        for app in all_apps() {
            assert_eq!(detected(app), tool_installed(app));
        }
    }

    #[test]
    fn binary_names_match_cc_switch() {
        assert_eq!(binary_names(AppId::Claude), &["claude"] as &[&str]);
        assert_eq!(binary_names(AppId::Codex), &["codex"] as &[&str]);
        assert_eq!(binary_names(AppId::OpenCode), &["opencode"] as &[&str]);
        assert_eq!(binary_names(AppId::Pi), &["pi"] as &[&str]);
    }
}
