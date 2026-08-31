use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static FAIL_BEFORE_RENAME: Cell<usize> = const { Cell::new(0) };
    static FAIL_STICKY: Cell<bool> = const { Cell::new(false) };
}

/// Panic if `path` would touch the host `~/.apmux` / `~/.claude` / `~/.codex` /
/// `~/.config/opencode` / `~/.pi`. Tests must inject tempfile `Paths`.
#[cfg(test)]
pub(crate) fn panic_if_host_config_path(path: &Path) {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let dirs = [
        home.join(".apmux"),
        home.join(".aimux"), // pre-rename dir must stay protected too
        home.join(".claude"),
        home.join(".codex"),
        home.join(".config").join("opencode"),
        home.join(".pi"),
    ];
    for dir in dirs {
        if path == dir || path.starts_with(&dir) {
            panic!(
                "tests must not write the host {} ({}); inject Paths with tempfile",
                dir.display(),
                path.display()
            );
        }
    }
}

#[cfg(test)]
pub(crate) fn fail_before_rename(fail: bool) {
    fail_before_rename_nth(if fail { 1 } else { 0 });
}

#[cfg(test)]
pub(crate) fn fail_before_rename_nth(n: usize) {
    FAIL_STICKY.with(|c| c.set(false));
    FAIL_BEFORE_RENAME.with(|c| c.set(n));
}

#[cfg(test)]
pub(crate) fn fail_before_rename_from_nth(n: usize) {
    FAIL_STICKY.with(|c| c.set(true));
    FAIL_BEFORE_RENAME.with(|c| c.set(n));
}

/// Create `path` (and parents). Unix mode `0700`; Windows chmod is a no-op.
pub fn ensure_dir_0700(path: &Path) -> Result<()> {
    #[cfg(test)]
    panic_if_host_config_path(path);

    fs::create_dir_all(path).with_context(|| format!("create dir {}", path.display()))?;
    chmod_dir_0700(path)?;
    Ok(())
}

pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    atomic_write_inner(path, data, true)
}

/// Atomic write for live secret files: new files are `0600`; existing mode is kept.
pub fn atomic_write_preserving_mode(path: &Path, data: &[u8]) -> Result<()> {
    atomic_write_inner(path, data, false)
}

fn atomic_write_inner(path: &Path, data: &[u8], force_0600: bool) -> Result<()> {
    #[cfg(test)]
    panic_if_host_config_path(path);

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| anyhow!("invalid path {}", path.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("invalid file name {}", path.display()))?;

    let existing_mode = if force_0600 {
        None
    } else {
        fs::metadata(path).ok().map(|m| m.permissions())
    };

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = parent.join(format!("{}.tmp.{ts}", file_name.to_string_lossy()));

    let result = write_tmp_and_rename(path, &tmp, data, existing_mode.is_none());
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
        return result;
    }
    if let Some(mode) = existing_mode {
        fs::set_permissions(path, mode)
            .with_context(|| format!("restore permissions {}", path.display()))?;
    }
    Ok(())
}

fn write_tmp_and_rename(path: &Path, tmp: &Path, data: &[u8], chmod_0600: bool) -> Result<()> {
    {
        let mut file = create_tmp(tmp)?;
        file.write_all(data)
            .with_context(|| format!("write {}", tmp.display()))?;
        file.flush()
            .with_context(|| format!("flush {}", tmp.display()))?;
    }

    #[cfg(test)]
    {
        let n = FAIL_BEFORE_RENAME.with(|c| c.get());
        if n == 1 {
            if !FAIL_STICKY.with(|c| c.get()) {
                FAIL_BEFORE_RENAME.with(|c| c.set(0));
            }
            anyhow::bail!("injected failure before rename");
        } else if n > 1 {
            FAIL_BEFORE_RENAME.with(|c| c.set(n - 1));
        }
    }

    rename_over(tmp, path)?;
    if chmod_0600 {
        chmod_file_0600(path)?;
    }
    Ok(())
}

fn create_tmp(tmp: &Path) -> Result<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(tmp)
            .with_context(|| format!("create {}", tmp.display()))
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(tmp)
            .with_context(|| format!("create {}", tmp.display()))
    }
}

fn rename_over(tmp: &Path, dest: &Path) -> Result<()> {
    // std::fs::rename replaces an existing dest on Windows (MoveFileExW +
    // MOVEFILE_REPLACE_EXISTING). Do not pre-delete: that plus tmp cleanup
    // on rename failure would drop both the old and new store.json.
    fs::rename(tmp, dest).map_err(|e| crate::error::Error::io(dest, e))?;
    Ok(())
}

pub(crate) fn chmod_file_0600(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 0600 {}", path.display()))?;
    }
    #[cfg(windows)]
    {
        let _ = path;
    }
    Ok(())
}

fn chmod_dir_0700(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("chmod 0700 {}", path.display()))?;
    }
    #[cfg(windows)]
    {
        let _ = path;
    }
    Ok(())
}

pub fn rename_replace(src: &Path, dest: &Path) -> Result<()> {
    #[cfg(test)]
    {
        panic_if_host_config_path(src);
        panic_if_host_config_path(dest);
    }
    fs::rename(src, dest).map_err(|e| crate::error::Error::io(dest, e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[cfg(unix)]
    fn unix_mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn atomic_write_replaces_and_cleans_tmp() {
        let td = tmp();
        let path = td.path().join("store.json");
        atomic_write(&path, b"one").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"one");
        atomic_write(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        let leftovers: Vec<_> = fs::read_dir(td.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .filter(|n| n.to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "tmp files left: {leftovers:?}");
    }

    #[test]
    fn injected_rename_failure_leaves_target_intact() {
        let td = tmp();
        let path = td.path().join("store.json");
        atomic_write(&path, b"old").unwrap();
        fail_before_rename(true);
        let err = atomic_write(&path, b"new").unwrap_err();
        assert!(err.to_string().contains("injected failure"));
        assert_eq!(fs::read(&path).unwrap(), b"old");
        let leftovers: Vec<_> = fs::read_dir(td.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .filter(|n| n.to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "tmp files left: {leftovers:?}");
    }

    #[cfg(unix)]
    #[test]
    fn unix_modes_0600_and_0700() {
        let td = tmp();
        let dir = td.path().join("cfg");
        ensure_dir_0700(&dir).unwrap();
        assert_eq!(unix_mode(&dir), 0o700);

        let path = dir.join("store.json");
        atomic_write(&path, b"{}").unwrap();
        assert_eq!(unix_mode(&path), 0o600);

        // chmod after save even if the file was world-readable beforehand.
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        atomic_write(&path, b"{}").unwrap();
        assert_eq!(unix_mode(&path), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn live_write_preserves_existing_mode_new_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let td = tmp();
        let path = td.path().join("settings.json");
        atomic_write_preserving_mode(&path, b"{}").unwrap();
        assert_eq!(unix_mode(&path), 0o600);

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        atomic_write_preserving_mode(&path, b"{\"a\":1}").unwrap();
        assert_eq!(unix_mode(&path), 0o644);
        assert_eq!(fs::read(&path).unwrap(), b"{\"a\":1}");
    }

    #[test]
    fn isolation_allows_temp_paths() {
        let td = tmp();
        panic_if_host_config_path(&td.path().join("store.json"));
    }

    #[test]
    fn isolation_rejects_host_config() {
        let home = dirs::home_dir().expect("home");
        let result = catch_unwind(AssertUnwindSafe(|| {
            panic_if_host_config_path(&home.join(".apmux").join("store.json"));
        }));
        assert!(result.is_err());
    }

    #[test]
    fn isolation_rejects_host_claude() {
        let home = dirs::home_dir().expect("home");
        let result = catch_unwind(AssertUnwindSafe(|| {
            panic_if_host_config_path(&home.join(".claude").join("settings.json"));
        }));
        assert!(result.is_err());
    }

    #[test]
    fn isolation_rejects_host_opencode() {
        let home = dirs::home_dir().expect("home");
        let result = catch_unwind(AssertUnwindSafe(|| {
            panic_if_host_config_path(&home.join(".config").join("opencode").join("opencode.json"));
        }));
        assert!(result.is_err());
    }

    #[test]
    fn isolation_rejects_host_codex() {
        let home = dirs::home_dir().expect("home");
        let result = catch_unwind(AssertUnwindSafe(|| {
            panic_if_host_config_path(&home.join(".codex").join("auth.json"));
        }));
        assert!(result.is_err());
    }

    #[test]
    fn isolation_rejects_host_pi() {
        let home = dirs::home_dir().expect("home");
        let result = catch_unwind(AssertUnwindSafe(|| {
            panic_if_host_config_path(&home.join(".pi").join("agent").join("models.json"));
        }));
        assert!(result.is_err());
    }

    #[test]
    fn atomic_write_to_host_config_panics() {
        let home = dirs::home_dir().expect("home");
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _ = atomic_write(&home.join(".apmux").join("store.json"), b"nope");
        }));
        assert!(result.is_err());
    }
}
