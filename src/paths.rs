use std::env;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use anyhow::{anyhow, Result};

/// Resolved directories for aimux and the target CLIs.
///
/// Production code uses [`Paths::from_env`]. Tests inject a tempfile home via
/// [`Paths::for_test`] / [`Paths::from_home_and_env`] and must never write the
/// host `~/.aimux`, `~/.claude`, `~/.codex`, `~/.config/opencode`, or `~/.pi`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub home: PathBuf,
    pub aimux_dir: PathBuf,
    pub claude_dir: PathBuf,
    pub codex_dir: PathBuf,
    pub opencode_dir: PathBuf,
    pub pi_dir: PathBuf,
}

/// Non-empty environment overrides. Empty strings are treated as unset.
#[derive(Debug, Default, Clone)]
pub struct EnvOverrides {
    pub aimux_config_dir: Option<String>,
    pub claude_config_dir: Option<String>,
    pub codex_home: Option<String>,
    pub xdg_config_home: Option<String>,
    pub pi_coding_agent_dir: Option<String>,
}

impl EnvOverrides {
    pub fn from_os() -> Self {
        Self {
            aimux_config_dir: nonempty_var(crate::name::ENV_CONFIG_DIR),
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

impl Paths {
    pub fn from_env() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("cannot determine home directory"))?;
        Self::from_home_and_env(home, EnvOverrides::from_os())
    }

    pub fn from_home_and_env(home: PathBuf, env: EnvOverrides) -> Result<Self> {
        let aimux_dir = match env.aimux_config_dir {
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
            aimux_dir,
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
        self.aimux_dir.join("store.json")
    }

    pub fn draft_file(&self) -> PathBuf {
        self.aimux_dir.join("providers.json")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.aimux_dir.join("backups")
    }

    pub fn webdav_file(&self) -> PathBuf {
        self.aimux_dir.join("webdav.json")
    }

    pub fn log_file(&self) -> PathBuf {
        self.aimux_dir.join(crate::name::LOG_FILE)
    }

    pub fn settings_file(&self) -> PathBuf {
        self.aimux_dir.join("settings.json")
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
            if self.aimux_dir == dir
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
    fn for_test_uses_injected_home() {
        let td = tmp();
        let p = Paths::for_test(td.path());
        assert_eq!(p.home, td.path());
        assert_eq!(p.aimux_dir, td.path().join(crate::name::DOT_DIR));
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
    fn aimux_config_dir_non_empty_wins() {
        let td = tmp();
        let custom = td.path().join("custom-aimux");
        let p = Paths::from_home_and_env(
            td.path().to_path_buf(),
            EnvOverrides {
                aimux_config_dir: Some(custom.display().to_string()),
                ..EnvOverrides::default()
            },
        )
        .unwrap();
        assert_eq!(p.aimux_dir, custom);
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
                aimux_config_dir: Some(td.path().join("x").display().to_string()),
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
