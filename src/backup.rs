use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};

use crate::error::Error;
use crate::fsutil;
use crate::paths::Paths;
use crate::store::Store;
use crate::switch;

/// Timestamp backups matching `YYYYMMDD_HHMMSS.json` are pruned to this many newest files.
pub(crate) const TIMESTAMP_RETAIN: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackupEntry {
    pub name: String,
    pub timestamp: bool,
}

/// Snapshot `store.json` into `$APMUX_CONFIG_DIR/backups/`. Live files are not copied.
/// `name = None` writes a local-time timestamp and rotates timestamp backups to 10.
pub(crate) fn create(paths: &Paths, name: Option<&str>) -> Result<String> {
    let mut store = Store::load(paths)?;
    // Snapshots capture user rows only; officials are re-seeded on load.
    store.providers.retain(|_, p| !p.official);
    let stem = match name {
        Some(raw) => named_stem(raw)?,
        None => timestamp_stem(),
    };
    let bytes = snapshot_bytes(paths, &store)?;

    fsutil::ensure_dir_0700(&paths.config_dir)?;
    let dir = paths.backups_dir();
    fsutil::ensure_dir_0700(&dir)?;

    let dest = dir.join(format!("{stem}.json"));
    fsutil::atomic_write(&dest, &bytes)?;
    log::info!("backup.write {}", dest.display());

    if name.is_none() {
        prune_timestamp_backups(&dir)?;
    }
    Ok(stem)
}

/// Restore `store.json` from a backup, then re-apply each `current[app]` unless `no_apply`.
pub(crate) fn restore(paths: &Paths, name: &str, yes: bool, no_apply: bool) -> Result<()> {
    restore_inner(paths, name, yes, no_apply, true)?;
    Ok(())
}

/// TUI restore: skip TTY confirm and do not `eprintln` re-apply warnings.
/// Returns apps that skipped live write (uninitialized).
pub(crate) fn restore_quiet(paths: &Paths, name: &str) -> Result<Vec<crate::store::AppId>> {
    restore_inner(paths, name, true, false, false)
}

fn restore_inner(
    paths: &Paths,
    name: &str,
    yes: bool,
    no_apply: bool,
    stderr_warn: bool,
) -> Result<Vec<crate::store::AppId>> {
    let stem = restore_stem(name)?;
    let src = paths.backups_dir().join(format!("{stem}.json"));
    if !src.is_file() {
        anyhow::bail!("backup not found: {stem}");
    }
    confirm_restore(&stem, yes)?;

    let mut store = Store::load_from_file(&src)?;
    if store.version < crate::store::STORE_VERSION {
        store.version = crate::store::STORE_VERSION;
    }
    store.ensure_official_providers();
    store
        .save(paths)
        .with_context(|| format!("restore store.json from {stem}"))?;

    if no_apply {
        return Ok(Vec::new());
    }
    if stderr_warn {
        switch::reapply_current(paths, &store)
            .with_context(|| format!("store restored from '{stem}' but re-apply failed"))?;
        Ok(Vec::new())
    } else {
        switch::reapply_current_quiet(paths, &store)
            .with_context(|| format!("store restored from '{stem}' but re-apply failed"))
    }
}

pub(crate) fn list(paths: &Paths) -> Result<Vec<BackupEntry>> {
    let dir = paths.backups_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut timestamps = Vec::new();
    let mut named = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| Error::io(&dir, e))? {
        let entry = entry.map_err(|e| Error::io(&dir, e))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(stem) = json_stem(&path) else {
            continue;
        };
        if is_timestamp_stem(&stem) {
            timestamps.push(BackupEntry {
                name: stem,
                timestamp: true,
            });
        } else {
            named.push(BackupEntry {
                name: stem,
                timestamp: false,
            });
        }
    }
    timestamps.sort_by(|a, b| b.name.cmp(&a.name));
    named.sort_by(|a, b| a.name.cmp(&b.name));
    timestamps.append(&mut named);
    Ok(timestamps)
}

fn snapshot_bytes(paths: &Paths, store: &Store) -> Result<Vec<u8>> {
    let file = paths.store_file();
    if file.exists() {
        fs::read(&file).map_err(|e| Error::io(&file, e).into())
    } else {
        let mut data = serde_json::to_string_pretty(store).context("serialize store.json")?;
        if !data.ends_with('\n') {
            data.push('\n');
        }
        Ok(data.into_bytes())
    }
}

fn timestamp_stem() -> String {
    chrono::Local::now().format("%Y%m%d_%H%M%S").to_string()
}

fn is_timestamp_stem(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 15
        && b[8] == b'_'
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[9..].iter().all(u8::is_ascii_digit)
}

fn json_stem(path: &Path) -> Option<String> {
    if path.extension().is_none_or(|ext| ext != "json") {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    if stem.is_empty() {
        return None;
    }
    Some(stem.to_string())
}

fn named_stem(raw: &str) -> Result<String> {
    let stem = strip_json_suffix(raw.trim());
    validate_stem(&stem)?;
    if is_timestamp_stem(&stem) {
        anyhow::bail!(
            "backup name '{stem}' looks like a timestamp; omit --name to create a timestamp backup"
        );
    }
    Ok(stem)
}

fn restore_stem(raw: &str) -> Result<String> {
    let stem = strip_json_suffix(raw.trim());
    validate_stem(&stem)?;
    Ok(stem)
}

fn strip_json_suffix(name: &str) -> String {
    name.strip_suffix(".json").unwrap_or(name).to_string()
}

fn validate_stem(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("backup name must not be empty");
    }
    if name.contains('/') || name.contains('\\') {
        anyhow::bail!("backup name must not contain '/'");
    }
    if name.contains("..") {
        anyhow::bail!("backup name must not contain '..'");
    }
    if name.chars().any(|c| c.is_control()) {
        anyhow::bail!("invalid backup name '{name}'");
    }
    let path = Path::new(name);
    match path.components().collect::<Vec<_>>().as_slice() {
        [Component::Normal(c)] if *c == name => Ok(()),
        _ => anyhow::bail!("invalid backup name '{name}'"),
    }
}

fn prune_timestamp_backups(dir: &Path) -> Result<usize> {
    let mut timestamps: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| Error::io(dir, e))? {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(stem) = json_stem(&path) else {
            continue;
        };
        if is_timestamp_stem(&stem) {
            timestamps.push(path);
        }
    }
    timestamps.sort();
    let mut removed = 0;
    if timestamps.len() > TIMESTAMP_RETAIN {
        let drop = timestamps.len() - TIMESTAMP_RETAIN;
        for path in timestamps.iter().take(drop) {
            fs::remove_file(path).map_err(|e| Error::io(path, e))?;
            removed += 1;
        }
    }
    if removed > 0 {
        log::info!("backup.prune removed={removed}");
    }
    Ok(removed)
}

fn confirm_restore(name: &str, yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !io::stdin().is_terminal() {
        anyhow::bail!("non-interactive restore requires --yes");
    }
    eprint!("Restore store.json from backup '{name}'? This overwrites the current store. [y/N] ");
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let ans = line.trim();
    if ans.eq_ignore_ascii_case("y") || ans.eq_ignore_ascii_case("yes") {
        Ok(())
    } else {
        anyhow::bail!("restore cancelled");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;
    use crate::store::{AppId, Provider, Store};
    use crate::switch::{self, AddOpts};
    use std::fs;

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

    fn save_packy(paths: &Paths, store: &mut Store, model: Option<&str>) -> String {
        let display = switch::add_provider(
            paths,
            store,
            AddOpts {
                app: AppId::Claude,
                name: "PackyCode".into(),
                base_url: "https://api.example.com".into(),
                api_key: "sk-test-key-abcd".into(),
                model: model.map(str::to_string),
                extra: vec![],
                catalog: vec![],
                slots: Default::default(),
                apply_snippet: false,
                snippet: None,
            },
        )
        .unwrap();
        switch::resolve(store, &display, None).unwrap().id.clone()
    }

    fn live_settings(paths: &Paths) -> PathBuf {
        paths.claude_dir.join("settings.json")
    }

    fn empty_store_json() -> Vec<u8> {
        let mut data = serde_json::to_string_pretty(&Store::empty()).unwrap();
        if !data.ends_with('\n') {
            data.push('\n');
        }
        data.into_bytes()
    }

    fn write_timestamp_files(dir: &Path, n: usize) {
        fsutil::ensure_dir_0700(dir).unwrap();
        for i in 1..=n {
            let path = dir.join(format!("20260101_{i:06}.json"));
            fsutil::atomic_write(&path, &empty_store_json()).unwrap();
        }
    }

    fn backup_names(paths: &Paths) -> Vec<String> {
        list(paths).unwrap().into_iter().map(|e| e.name).collect()
    }

    #[test]
    fn named_backup_writes_store_only_with_0600() {
        let (_td, paths) = setup();
        let mut store = Store::empty();
        save_packy(&paths, &mut store, None);
        fs::create_dir_all(&paths.claude_dir).unwrap();
        fs::write(live_settings(&paths), b"{\"permissions\":{}}").unwrap();

        let stem = create(&paths, Some("before-migrate")).unwrap();
        assert_eq!(stem, "before-migrate");
        let dest = paths.backups_dir().join("before-migrate.json");
        assert!(dest.is_file());
        let parsed = Store::load_from_file(&dest).unwrap();
        assert!(parsed.providers.contains_key("packycode"));

        let extras: Vec<_> = fs::read_dir(paths.backups_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .collect();
        assert_eq!(extras.len(), 1, "must not copy live files: {extras:?}");

        #[cfg(unix)]
        {
            assert_eq!(unix_mode(&paths.backups_dir()), 0o700);
            assert_eq!(unix_mode(&dest), 0o600);
        }
        crate::fsutil::panic_if_host_config_path(&dest);
    }

    #[test]
    fn timestamp_backup_name_matches_local_pattern() {
        let (_td, paths) = setup();
        Store::empty().save(&paths).unwrap();
        let stem = create(&paths, None).unwrap();
        assert!(is_timestamp_stem(&stem), "{stem}");
        assert!(paths.backups_dir().join(format!("{stem}.json")).is_file());
    }

    #[test]
    fn named_timestamp_is_rejected() {
        let (_td, paths) = setup();
        let err = create(&paths, Some("20260825_101500")).unwrap_err();
        assert!(err.to_string().contains("looks like a timestamp"), "{err}");
        let err = create(&paths, Some("20260825_101500.json")).unwrap_err();
        assert!(err.to_string().contains("looks like a timestamp"), "{err}");
        assert!(!paths.backups_dir().exists() || list(&paths).unwrap().is_empty());
    }

    #[test]
    fn named_rejects_slash_dotdot_empty() {
        let (_td, paths) = setup();
        for bad in [
            "", "  ", "/", "foo/bar", "..", "foo..bar", "../x", "foo\\bar",
        ] {
            let err = create(&paths, Some(bad)).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("empty")
                    || msg.contains("'/'")
                    || msg.contains("'..'")
                    || msg.contains("invalid"),
                "name {bad:?} -> {msg}"
            );
        }
    }

    #[test]
    fn rotate_keeps_ten_timestamps_and_named() {
        let (_td, paths) = setup();
        Store::empty().save(&paths).unwrap();
        write_timestamp_files(&paths.backups_dir(), 10);
        fsutil::atomic_write(
            &paths.backups_dir().join("before-migrate.json"),
            &empty_store_json(),
        )
        .unwrap();

        let stem = create(&paths, None).unwrap();
        let entries = list(&paths).unwrap();
        let ts: Vec<_> = entries.iter().filter(|e| e.timestamp).collect();
        let named: Vec<_> = entries.iter().filter(|e| !e.timestamp).collect();
        assert_eq!(ts.len(), 10);
        assert_eq!(named.len(), 1);
        assert_eq!(named[0].name, "before-migrate");
        assert!(ts.iter().any(|e| e.name == stem));
        assert!(!paths.backups_dir().join("20260101_000001.json").exists());
        assert!(paths.backups_dir().join("before-migrate.json").exists());
    }

    #[test]
    fn eleven_timestamps_prune_to_ten() {
        let (_td, paths) = setup();
        write_timestamp_files(&paths.backups_dir(), 11);
        fsutil::atomic_write(
            &paths.backups_dir().join("keep-me.json"),
            &empty_store_json(),
        )
        .unwrap();
        prune_timestamp_backups(&paths.backups_dir()).unwrap();
        let entries = list(&paths).unwrap();
        assert_eq!(entries.iter().filter(|e| e.timestamp).count(), 10);
        assert!(entries.iter().any(|e| e.name == "keep-me"));
        assert!(!paths.backups_dir().join("20260101_000001.json").exists());
        assert!(paths.backups_dir().join("20260101_000011.json").exists());
    }

    #[test]
    fn named_backup_is_not_rotated() {
        let (_td, paths) = setup();
        Store::empty().save(&paths).unwrap();
        write_timestamp_files(&paths.backups_dir(), 10);
        create(&paths, Some("snap")).unwrap();
        assert_eq!(
            list(&paths).unwrap().iter().filter(|e| e.timestamp).count(),
            10
        );
        assert!(paths.backups_dir().join("snap.json").exists());
        assert!(paths.backups_dir().join("20260101_000001.json").exists());
    }

    #[test]
    fn restore_reapplies_current_to_live() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut store = Store::empty();
        let packy = save_packy(&paths, &mut store, Some("sonnet"));
        switch::use_provider(&paths, &mut store, &packy, None).unwrap();
        create(&paths, Some("snap")).unwrap();

        let other = switch::add_provider(
            &paths,
            &mut store,
            AddOpts {
                app: AppId::Claude,
                name: "Other".into(),
                base_url: "https://other.example.com".into(),
                api_key: "sk-other-key-zzzz".into(),
                model: Some("opus".into()),
                extra: vec![],
                catalog: vec![],
                slots: Default::default(),
                apply_snippet: false,
                snippet: None,
            },
        )
        .unwrap();
        switch::use_provider(&paths, &mut store, &other, None).unwrap();
        let after_switch: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(live_settings(&paths)).unwrap()).unwrap();
        assert_eq!(after_switch["env"]["ANTHROPIC_MODEL"], "opus");

        restore(&paths, "snap", true, false).unwrap();
        let loaded = Store::load(&paths).unwrap();
        assert_eq!(
            loaded.current.get(&AppId::Claude).map(String::as_str),
            Some("packycode")
        );
        let live: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(live_settings(&paths)).unwrap()).unwrap();
        assert_eq!(live["env"]["ANTHROPIC_MODEL"], "sonnet");
        assert_eq!(live["env"]["ANTHROPIC_BASE_URL"], "https://api.example.com");
    }

    #[test]
    fn restore_no_apply_leaves_live() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut store = Store::empty();
        let packy = save_packy(&paths, &mut store, Some("sonnet"));
        switch::use_provider(&paths, &mut store, &packy, None).unwrap();
        create(&paths, Some("snap")).unwrap();

        let other = switch::add_provider(
            &paths,
            &mut store,
            AddOpts {
                app: AppId::Claude,
                name: "Other".into(),
                base_url: "https://other.example.com".into(),
                api_key: "sk-other-key-zzzz".into(),
                model: Some("opus".into()),
                extra: vec![],
                catalog: vec![],
                slots: Default::default(),
                apply_snippet: false,
                snippet: None,
            },
        )
        .unwrap();
        switch::use_provider(&paths, &mut store, &other, None).unwrap();
        restore(&paths, "snap", true, true).unwrap();
        let loaded = Store::load(&paths).unwrap();
        assert_eq!(
            loaded.current.get(&AppId::Claude).map(String::as_str),
            Some("packycode")
        );
        let live: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(live_settings(&paths)).unwrap()).unwrap();
        assert_eq!(live["env"]["ANTHROPIC_MODEL"], "opus");
    }

    #[test]
    fn restore_uninitialized_skips_live_like_use() {
        let (_td, paths) = setup();
        let mut store = Store::empty();
        let packy = save_packy(&paths, &mut store, None);
        switch::use_provider(&paths, &mut store, &packy, None).unwrap();
        create(&paths, Some("snap")).unwrap();
        restore(&paths, "snap", true, false).unwrap();
        assert!(!paths.claude_dir.exists());
        assert!(!live_settings(&paths).exists());
        let loaded = Store::load(&paths).unwrap();
        assert_eq!(
            loaded.current.get(&AppId::Claude).map(String::as_str),
            Some("packycode")
        );
    }

    #[test]
    fn restore_missing_current_id_skips_and_continues() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut store = Store::empty();
        let packy = save_packy(&paths, &mut store, Some("sonnet"));
        store.current.insert(AppId::Claude, packy.clone());
        store.current.insert(AppId::Codex, "gone".into());
        store.save(&paths).unwrap();
        create(&paths, Some("snap")).unwrap();

        store.current.insert(AppId::Claude, "other".into());
        store.save(&paths).unwrap();

        restore(&paths, "snap", true, false).unwrap();
        let live: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(live_settings(&paths)).unwrap()).unwrap();
        assert_eq!(live["env"]["ANTHROPIC_MODEL"], "sonnet");
        let loaded = Store::load(&paths).unwrap();
        assert_eq!(
            loaded.current.get(&AppId::Codex).map(String::as_str),
            Some("gone")
        );
    }

    #[test]
    fn restore_unimplemented_current_does_not_abort_claude() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut store = Store::empty();
        let packy = save_packy(&paths, &mut store, Some("sonnet"));
        store.providers.insert(
            "cx".into(),
            Provider {
                id: "cx".into(),
                name: "Codex".into(),
                app: AppId::Codex,
                base_url: "https://example.com".into(),
                api_key: "k".into(),
                model: None,
                extras: Default::default(),
                ..Provider::blank(AppId::Codex)
            },
        );
        store.current.insert(AppId::Claude, packy);
        store.current.insert(AppId::Codex, "cx".into());
        store.save(&paths).unwrap();
        create(&paths, Some("snap")).unwrap();

        store.current.remove(&AppId::Claude);
        store.save(&paths).unwrap();

        restore(&paths, "snap", true, false).unwrap();
        assert!(!paths.codex_dir.exists());
        let live: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(live_settings(&paths)).unwrap()).unwrap();
        assert_eq!(live["env"]["ANTHROPIC_MODEL"], "sonnet");
        let loaded = Store::load(&paths).unwrap();
        assert_eq!(
            loaded.current.get(&AppId::Claude).map(String::as_str),
            Some("packycode")
        );
        assert_eq!(
            loaded.current.get(&AppId::Codex).map(String::as_str),
            Some("cx")
        );
    }

    #[test]
    fn restore_corrupt_live_fails_that_app_store_already_restored() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut store = Store::empty();
        let packy = save_packy(&paths, &mut store, Some("sonnet"));
        switch::use_provider(&paths, &mut store, &packy, None).unwrap();
        create(&paths, Some("snap")).unwrap();

        store.providers.insert(
            "cx".into(),
            Provider {
                id: "cx".into(),
                name: "Codex".into(),
                app: AppId::Codex,
                base_url: "https://example.com".into(),
                api_key: "k".into(),
                model: None,
                extras: Default::default(),
                ..Provider::blank(AppId::Codex)
            },
        );
        store.current.insert(AppId::Codex, "cx".into());
        store.current.insert(AppId::Claude, "changed".into());
        store.save(&paths).unwrap();

        let live = live_settings(&paths);
        let corrupt = fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/golden/claude/corrupt.json"),
        )
        .unwrap();
        fs::write(&live, &corrupt).unwrap();

        let err = restore(&paths, "snap", true, false).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("re-apply failed") || msg.contains("settings.json"),
            "{msg}"
        );
        assert_eq!(fs::read(&live).unwrap(), corrupt);
        let loaded = Store::load(&paths).unwrap();
        assert_eq!(
            loaded.current.get(&AppId::Claude).map(String::as_str),
            Some("packycode")
        );
        assert!(!loaded.current.contains_key(&AppId::Codex));
    }

    #[test]
    fn restore_corrupt_does_not_abort_other_current() {
        let (_td, paths) = setup();
        fs::create_dir_all(&paths.claude_dir).unwrap();
        let mut store = Store::empty();
        let packy = save_packy(&paths, &mut store, Some("sonnet"));
        store.current.insert(AppId::Claude, packy);
        store.current.insert(AppId::Pi, "missing-pi".into());
        store.save(&paths).unwrap();
        create(&paths, Some("snap")).unwrap();

        let live = live_settings(&paths);
        let corrupt = fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/golden/claude/corrupt.json"),
        )
        .unwrap();
        fs::write(&live, &corrupt).unwrap();

        let err = restore(&paths, "snap", true, false).unwrap_err();
        assert!(err.to_string().contains("re-apply failed"), "{err}");
        assert_eq!(fs::read(&live).unwrap(), corrupt);
        let loaded = Store::load(&paths).unwrap();
        assert_eq!(
            loaded.current.get(&AppId::Pi).map(String::as_str),
            Some("missing-pi")
        );
    }

    #[test]
    fn restore_without_yes_on_non_tty_fails() {
        let (_td, paths) = setup();
        let mut store = Store::empty();
        save_packy(&paths, &mut store, None);
        create(&paths, Some("snap")).unwrap();
        let before = fs::read(paths.store_file()).unwrap();
        let err = restore(&paths, "snap", false, false).unwrap_err();
        assert!(err.to_string().contains("--yes"), "{err}");
        assert_eq!(fs::read(paths.store_file()).unwrap(), before);
    }

    #[test]
    fn restore_missing_backup_does_not_touch_store() {
        let (_td, paths) = setup();
        let mut store = Store::empty();
        save_packy(&paths, &mut store, None);
        let before = fs::read(paths.store_file()).unwrap();
        let err = restore(&paths, "nope", true, true).unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
        assert_eq!(fs::read(paths.store_file()).unwrap(), before);
    }

    #[test]
    fn restore_invalid_or_future_backup_does_not_touch_store() {
        let (_td, paths) = setup();
        let mut store = Store::empty();
        save_packy(&paths, &mut store, None);
        let before = fs::read(paths.store_file()).unwrap();
        fsutil::ensure_dir_0700(&paths.backups_dir()).unwrap();
        fs::write(paths.backups_dir().join("bad.json"), b"{not json").unwrap();
        let err = restore(&paths, "bad", true, true).unwrap_err();
        assert!(
            err.to_string().contains("parse") || err.to_string().contains("bad.json"),
            "{err}"
        );
        assert_eq!(fs::read(paths.store_file()).unwrap(), before);

        fs::write(
            paths.backups_dir().join("future.json"),
            r#"{"version":99,"current":{},"providers":{}}"#,
        )
        .unwrap();
        let err = restore(&paths, "future", true, true).unwrap_err();
        assert!(
            err.to_string().contains("version") || err.to_string().contains("99"),
            "{err}"
        );
        assert_eq!(fs::read(paths.store_file()).unwrap(), before);
    }

    #[test]
    fn list_empty_when_dir_missing() {
        let (_td, paths) = setup();
        assert!(backup_names(&paths).is_empty());
    }

    #[test]
    fn restore_accepts_json_suffix() {
        let (_td, paths) = setup();
        let mut store = Store::empty();
        save_packy(&paths, &mut store, None);
        create(&paths, Some("snap")).unwrap();
        restore(&paths, "snap.json", true, true).unwrap();
        assert!(Store::load(&paths)
            .unwrap()
            .providers
            .contains_key("packycode"));
    }

    #[test]
    fn empty_store_can_be_backed_up() {
        let (_td, paths) = setup();
        let stem = create(&paths, Some("empty")).unwrap();
        assert_eq!(stem, "empty");
        let mut parsed = Store::load_from_file(&paths.backups_dir().join("empty.json")).unwrap();
        // load_from_file does not seed; officials only appear via Store::load.
        assert_eq!(parsed, Store::empty());
        parsed.ensure_official_providers();
        assert!(parsed.providers.values().all(|p| p.official));
    }
}
