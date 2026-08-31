use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::name;

/// Resolved directories for apmux and the target CLIs.
///
/// Production code uses [`Paths::from_env`]. Tests inject a tempfile home via
/// [`Paths::for_test`] / [`Paths::from_home_and_env`] and must never write the
/// host `~/.apmux`, `~/.claude`, `~/.codex`, `~/.config/opencode`, or `~/.pi`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub home: PathBuf,
    pub config_dir: PathBuf,
    pub claude_dir: PathBuf,
    pub codex_dir: PathBuf,
    pub opencode_dir: PathBuf,
    pub pi_dir: PathBuf,
}

/// Non-empty environment overrides. Empty strings are treated as unset.
#[derive(Debug, Default, Clone)]
pub struct EnvOverrides {
    pub config_dir_override: Option<String>,
    pub claude_config_dir: Option<String>,
    pub codex_home: Option<String>,
    pub xdg_config_home: Option<String>,
    pub pi_coding_agent_dir: Option<String>,
}

impl EnvOverrides {
    pub fn from_os() -> Self {
        Self {
            config_dir_override: nonempty_var(name::ENV_CONFIG_DIR)
                .or_else(|| nonempty_var(name::LEGACY_ENV_CONFIG_DIR)),
            claude_config_dir: nonempty_var("CLAUDE_CONFIG_DIR"),
            codex_home: nonempty_var("CODEX_HOME"),
            xdg_config_home: nonempty_var("XDG_CONFIG_HOME"),
            pi_coding_agent_dir: nonempty_var("PI_CODING_AGENT_DIR"),
        }
    }
}

fn nonempty_var(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

fn copy_missing_recursive(src: &Path, dst: &Path, copied: &mut usize) -> Result<()> {
    for entry in fs::read_dir(src).with_context(|| format!("read_dir {}", src.display()))? {
        let entry = entry.with_context(|| format!("read_dir entry in {}", src.display()))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ft = entry
            .file_type()
            .with_context(|| format!("file_type {}", from.display()))?;
        if ft.is_dir() {
            if !to.exists() {
                fs::create_dir_all(&to)
                    .with_context(|| format!("create_dir_all {}", to.display()))?;
            }
            copy_missing_recursive(&from, &to, copied)?;
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        if to.exists() {
            continue;
        }
        fs::copy(&from, &to)
            .with_context(|| format!("copy legacy {} → {}", from.display(), to.display()))?;
        *copied += 1;
    }
    Ok(())
}

impl Paths {
    /// One-time migration of the pre-rename config directory
    /// (`~/.aimux` → `~/.apmux`). Only the default location is migrated;
    /// explicit `*_CONFIG_DIR` overrides are left untouched.
    ///
    /// Cases:
    ///  - `old` absent → noop (clean install or already migrated)
    ///  - `old` present, `new` absent → atomic rename
    ///  - both present → merge: copy files from `old` into `new` only when
    ///    `new` does not already have a same-named entry; print a warning so
    ///    the user can clean up `old` by hand. We do not rename or delete
    ///    `old` in this case because the new directory may already contain
    ///    authoritative state (e.g. a fresh store written before the legacy
    ///    webdav.json could be picked up — the bug this guard exists to fix).
    pub fn migrate_legacy_dir() -> Result<()> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("cannot determine home directory"))?;
        Self::migrate_legacy_dir_homed(&home)
    }

    /// Testable core of [`Self::migrate_legacy_dir`].
    pub fn migrate_legacy_dir_homed(home: &std::path::Path) -> Result<()> {
        let old = home.join(name::LEGACY_DOT_DIR);
        let new = home.join(name::DOT_DIR);
        if !old.exists() {
            return Ok(());
        }
        if !new.exists() {
            fs::rename(&old, &new).with_context(|| {
                format!("migrate config dir {} → {}", old.display(), new.display())
            })?;
            println!(
                "migrated config directory {} → {}",
                old.display(),
                new.display()
            );
            return Ok(());
        }
        // Both exist: bring missing entries across and warn the user. We
        // never overwrite an existing entry in `new` because the new
        // directory may already hold authoritative state.
        let mut copied = 0usize;
        copy_missing_recursive(&old, &new, &mut copied)?;
        if copied > 0 {
            eprintln!(
                "apmux: merged {copied} missing file(s) from legacy config dir {} into {}; \
                 you can remove {} once you confirm everything looks right.",
                old.display(),
                new.display(),
                old.display()
            );
        } else {
            eprintln!(
                "apmux: legacy config dir {} left in place — every entry it contains \
                 already has a counterpart in {}. Remove it manually if it is no longer needed.",
                old.display(),
                new.display()
            );
        }
        Ok(())
    }

    pub fn from_env() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("cannot determine home directory"))?;
        Self::from_home_and_env(home, EnvOverrides::from_os())
    }

    pub fn from_home_and_env(home: PathBuf, env: EnvOverrides) -> Result<Self> {
        let config_dir = match env.config_dir_override {
            Some(dir) => PathBuf::from(dir),
            None => home.join(crate::name::DOT_DIR),
        };
        let claude_dir = match env.claude_config_dir {
            Some(dir) => PathBuf::from(dir),
            None => home.join(".claude"),
        };
        let codex_dir = match env.codex_home {
            Some(dir) => {
                let p = PathBuf::from(dir);
                if p.is_dir() {
                    p
                } else {
                    home.join(".codex")
                }
            }
            None => home.join(".codex"),
        };
        let opencode_dir = match env.xdg_config_home {
            Some(dir) => PathBuf::from(dir).join("opencode"),
            None => home.join(".config").join("opencode"),
        };
        let pi_dir = match env.pi_coding_agent_dir {
            Some(dir) => PathBuf::from(dir),
            None => home.join(".pi").join("agent"),
        };

        let paths = Self {
            home,
            config_dir,
            claude_dir,
            codex_dir,
            opencode_dir,
            pi_dir,
        };
        #[cfg(test)]
        paths.assert_isolated();
        Ok(paths)
    }

    /// Test helper: all CLI dirs live under `root` (typically a `tempfile` dir).
    #[cfg(test)]
    pub fn for_test(root: &Path) -> Self {
        Self::from_home_and_env(root.to_path_buf(), EnvOverrides::default())
            .unwrap_or_else(|e| panic!("Paths::for_test: {e}"))
    }

    pub fn store_file(&self) -> PathBuf {
        self.config_dir.join("store.json")
    }

    pub fn draft_file(&self) -> PathBuf {
        self.config_dir.join("providers.json")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.config_dir.join("backups")
    }

    pub fn webdav_file(&self) -> PathBuf {
        self.config_dir.join("webdav.json")
    }

    pub fn log_file(&self) -> PathBuf {
        self.config_dir.join(crate::name::LOG_FILE)
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    #[cfg(test)]
    pub(crate) fn assert_isolated(&self) {
        let Some(real_home) = dirs::home_dir() else {
            return;
        };
        if self.home == real_home {
            panic!(
                "tests must inject Paths (temp dirs), not Paths::from_env() against the host home {}",
                real_home.display()
            );
        }
        let host = [
            ("~/.aimux", real_home.join(crate::name::DOT_DIR)),
            ("~/.claude", real_home.join(".claude")),
            ("~/.codex", real_home.join(".codex")),
            (
                "~/.config/opencode",
                real_home.join(".config").join("opencode"),
            ),
            ("~/.pi", real_home.join(".pi")),
        ];
        for (label, dir) in host {
            if self.config_dir == dir
                || self.claude_dir == dir
                || self.codex_dir == dir
                || self.opencode_dir == dir
                || self.pi_dir == dir
                || self.pi_dir == dir.join("agent")
            {
                panic!(
                    "tests must not use the host {label} ({}); inject Paths with tempfile",
                    dir.display()
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn migrate_moves_legacy_dir_into_dot_dir() {
        let td = tmp();
        let old = td.path().join(name::LEGACY_DOT_DIR);
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("store.json"), "{}").unwrap();
        assert!(!td.path().join(name::DOT_DIR).exists());
        Paths::migrate_legacy_dir_homed(td.path()).unwrap();
        assert!(!old.exists());
        assert!(td.path().join(name::DOT_DIR).join("store.json").exists());
    }

    #[test]
    fn migrate_merges_missing_files_when_both_dirs_exist() {
        let td = tmp();
        let old = td.path().join(name::LEGACY_DOT_DIR);
        let new = td.path().join(name::DOT_DIR);
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        // common file already in new — must NOT be overwritten
        fs::write(old.join("store.json"), "old store").unwrap();
        fs::write(new.join("store.json"), "new store").unwrap();
        // file only in old — must be copied across
        fs::write(old.join("webdav.json"), "{\"url\":\"x\"}").unwrap();
        // subdir only in old — must be copied across
        fs::create_dir_all(old.join("backups")).unwrap();
        fs::write(old.join("backups").join("b.json"), "{}").unwrap();
        // file only in new — must stay
        fs::write(new.join("settings.json"), "{\"lang\":\"en\"}").unwrap();

        Paths::migrate_legacy_dir_homed(td.path()).unwrap();

        // authoritative file preserved
        assert_eq!(
            fs::read_to_string(new.join("store.json")).unwrap(),
            "new store"
        );
        // missing file copied
        assert_eq!(
            fs::read_to_string(new.join("webdav.json")).unwrap(),
            "{\"url\":\"x\"}"
        );
        // nested file copied
        assert_eq!(
            fs::read_to_string(new.join("backups").join("b.json")).unwrap(),
            "{}"
        );
        // new-only file untouched
        assert_eq!(
            fs::read_to_string(new.join("settings.json")).unwrap(),
            "{\"lang\":\"en\"}"
        );
        // old dir left in place for user cleanup
        assert!(old.exists());
    }

    #[test]
    fn migrate_is_noop_when_legacy_dir_absent() {
        let td = tmp();
        Paths::migrate_legacy_dir_homed(td.path()).unwrap();
        assert!(!td.path().join(name::DOT_DIR).exists());
    }

    #[test]
    fn for_test_uses_injected_home() {
        let td = tmp();
        let p = Paths::for_test(td.path());
        assert_eq!(p.home, td.path());
        assert_eq!(p.config_dir, td.path().join(crate::name::DOT_DIR));
        assert_eq!(p.claude_dir, td.path().join(".claude"));
        assert_eq!(p.codex_dir, td.path().join(".codex"));
        assert_eq!(p.opencode_dir, td.path().join(".config").join("opencode"));
        assert_eq!(p.pi_dir, td.path().join(".pi").join("agent"));
        assert_eq!(
            p.webdav_file(),
            td.path().join(crate::name::DOT_DIR).join("webdav.json")
        );
        assert_eq!(
            p.log_file(),
            td.path()
                .join(crate::name::DOT_DIR)
                .join(crate::name::LOG_FILE)
        );
    }

    #[test]
    fn config_dir_override_non_empty_wins() {
        let td = tmp();
        let custom = td.path().join("custom-cfg");
        let p = Paths::from_home_and_env(
            td.path().to_path_buf(),
            EnvOverrides {
                config_dir_override: Some(custom.display().to_string()),
                ..EnvOverrides::default()
            },
        )
        .unwrap();
        assert_eq!(p.config_dir, custom);
    }

    #[test]
    fn claude_config_dir_non_empty_wins_even_if_missing() {
        let td = tmp();
        let missing = td.path().join("missing-claude");
        assert!(!missing.exists());
        let p = Paths::from_home_and_env(
            td.path().to_path_buf(),
            EnvOverrides {
                claude_config_dir: Some(missing.display().to_string()),
                ..EnvOverrides::default()
            },
        )
        .unwrap();
        assert_eq!(p.claude_dir, missing);
    }

    #[test]
    fn codex_home_adopted_only_if_dir() {
        let td = tmp();
        let real = td.path().join("codex-real");
        fs::create_dir_all(&real).unwrap();
        let p = Paths::from_home_and_env(
            td.path().to_path_buf(),
            EnvOverrides {
                codex_home: Some(real.display().to_string()),
                ..EnvOverrides::default()
            },
        )
        .unwrap();
        assert_eq!(p.codex_dir, real);

        let not_dir = td.path().join("codex-file");
        fs::write(&not_dir, b"x").unwrap();
        let p = Paths::from_home_and_env(
            td.path().to_path_buf(),
            EnvOverrides {
                codex_home: Some(not_dir.display().to_string()),
                ..EnvOverrides::default()
            },
        )
        .unwrap();
        assert_eq!(p.codex_dir, td.path().join(".codex"));

        let missing = td.path().join("codex-missing");
        let p = Paths::from_home_and_env(
            td.path().to_path_buf(),
            EnvOverrides {
                codex_home: Some(missing.display().to_string()),
                ..EnvOverrides::default()
            },
        )
        .unwrap();
        assert_eq!(p.codex_dir, td.path().join(".codex"));
    }

    #[test]
    fn pi_dir_non_empty_wins_even_if_missing() {
        let td = tmp();
        let missing = td.path().join("missing-pi");
        let p = Paths::from_home_and_env(
            td.path().to_path_buf(),
            EnvOverrides {
                pi_coding_agent_dir: Some(missing.display().to_string()),
                ..EnvOverrides::default()
            },
        )
        .unwrap();
        assert_eq!(p.pi_dir, missing);
    }

    #[test]
    fn opencode_defaults_to_home_config_opencode() {
        let td = tmp();
        let p = Paths::from_home_and_env(
            td.path().to_path_buf(),
            EnvOverrides {
                config_dir_override: Some(td.path().join("x").display().to_string()),
                claude_config_dir: Some(td.path().join("c").display().to_string()),
                pi_coding_agent_dir: Some(td.path().join("p").display().to_string()),
                ..EnvOverrides::default()
            },
        )
        .unwrap();
        assert_eq!(p.opencode_dir, td.path().join(".config").join("opencode"));
    }

    #[test]
    fn xdg_config_home_non_empty_wins_even_if_missing() {
        let td = tmp();
        let missing = td.path().join("xdg-missing");
        assert!(!missing.exists());
        let p = Paths::from_home_and_env(
            td.path().to_path_buf(),
            EnvOverrides {
                xdg_config_home: Some(missing.display().to_string()),
                ..EnvOverrides::default()
            },
        )
        .unwrap();
        assert_eq!(p.opencode_dir, missing.join("opencode"));
        assert_ne!(p.opencode_dir, td.path().join(".config").join("opencode"));
    }

    #[test]
    fn from_env_panics_in_tests_against_real_home() {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = Paths::from_env();
        }));
        assert!(
            result.is_err(),
            "from_env() must panic in tests so we never touch the host home"
        );
    }

    #[test]
    fn for_test_on_real_home_panics() {
        let real = dirs::home_dir().expect("home");
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = Paths::for_test(&real);
        }));
        assert!(result.is_err());
    }

    #[test]
    fn host_opencode_dir_panics() {
        let td = tmp();
        let real = dirs::home_dir().expect("home");
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = Paths::from_home_and_env(
                td.path().to_path_buf(),
                EnvOverrides {
                    xdg_config_home: Some(real.join(".config").display().to_string()),
                    ..EnvOverrides::default()
                },
            );
        }));
        assert!(result.is_err());
    }
}
