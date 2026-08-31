use std::fs;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Error;
use crate::fsutil;
use crate::name;
use crate::paths::Paths;
use crate::store::Store;
use crate::webdav::{self, DavClient};
use crate::{backup, switch};

const MANIFEST_FORMAT: &str = crate::name::MANIFEST_FORMAT;
const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct WebDavConfig {
    url: String,
    username: String,
    password: String,
    #[serde(default)]
    last_pulled_sha256: String,
    #[serde(default)]
    last_pushed_sha256: String,
    #[serde(default)]
    last_sync_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    created_at: String,
    device: String,
    bytes: u64,
    sha256: String,
}

pub(crate) fn setup(paths: &Paths, url: String, username: String, password: String) -> Result<()> {
    if username.is_empty() {
        anyhow::bail!("username must not be empty");
    }
    if password.is_empty() {
        anyhow::bail!("password must not be empty");
    }
    let url = webdav::validate_remote_url(&url)?;
    let collection = webdav::namespaced_collection(&url)?;
    let mut cfg = load_config(paths).unwrap_or(WebDavConfig {
        url: String::new(),
        username: String::new(),
        password: String::new(),
        last_pulled_sha256: String::new(),
        last_pushed_sha256: String::new(),
        last_sync_at: String::new(),
    });
    let user = username.clone();
    let pass = password.clone();
    webdav::block_on(async move {
        let client = DavClient::new(&user, &pass)?;
        client.ensure_remote_directories(&collection).await
    })?;
    if cfg.url != url {
        cfg.last_pulled_sha256.clear();
        cfg.last_pushed_sha256.clear();
        cfg.last_sync_at.clear();
    }
    cfg.url = url;
    cfg.username = username;
    cfg.password = password;
    save_config(paths, &cfg)
}

pub(crate) fn push(paths: &Paths, force: bool) -> Result<String> {
    let mut cfg = load_config(paths)?;
    let local_bytes = local_store_bytes(paths)?;
    let local_sha = sha256_hex(&local_bytes);

    let username = cfg.username.clone();
    let password = cfg.password.clone();
    let collection = webdav::namespaced_collection(&cfg.url)?;
    let last_pulled = cfg.last_pulled_sha256.clone();
    let sha_check = local_sha.clone();
    webdav::block_on(async move {
        let client = DavClient::new(&username, &password)?;
        client.ensure_remote_directories(&collection).await?;
        let manifest_url = webdav::join_file(&collection, "manifest.json")?;
        let remote_sha = match client.get(&manifest_url).await? {
            None => String::new(),
            Some(body) => match parse_manifest(&body) {
                Ok(m) => m.sha256,
                Err(_) if force => {
                    log::info!("webdav.conflict unreadable remote manifest; --force overwrites");
                    String::new()
                }
                Err(e) => {
                    anyhow::bail!(
                        "remote manifest.json is unreadable ({e:#}); pass --force to overwrite"
                    );
                }
            },
        };
        if push_conflict(&remote_sha, &last_pulled, &sha_check) && !force {
            log::info!("webdav.conflict sha={remote_sha}");
            anyhow::bail!("remote store has changed; run apmux sync pull or pass --force");
        }
        Ok(())
    })?;

    backup::create(paths, None)?;

    let username = cfg.username.clone();
    let password = cfg.password.clone();
    let collection = webdav::namespaced_collection(&cfg.url)?;
    let sha_put = local_sha.clone();
    let body = local_bytes;
    webdav::block_on(async move {
        let client = DavClient::new(&username, &password)?;
        let store_url = webdav::join_file(&collection, "store.json")?;
        let manifest_url = webdav::join_file(&collection, "manifest.json")?;
        client.put(&store_url, &body).await?;
        let manifest = Manifest {
            format: MANIFEST_FORMAT.to_string(),
            version: MANIFEST_VERSION,
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            device: device_name(),
            bytes: body.len() as u64,
            sha256: sha_put,
        };
        let man_bytes = serde_json::to_vec_pretty(&manifest).context("serialize manifest.json")?;
        client.put(&manifest_url, &man_bytes).await?;
        Ok(())
    })?;

    cfg.last_pulled_sha256 = local_sha.clone();
    cfg.last_pushed_sha256 = local_sha.clone();
    cfg.last_sync_at = now_local();
    save_config(paths, &cfg)?;
    log::info!("webdav.push sha={local_sha}");
    Ok(local_sha)
}

pub(crate) fn pull(paths: &Paths, force: bool) -> Result<String> {
    pull_inner(paths, force, true)
}

/// Same as [`pull`] but does not `eprintln` re-apply warnings (TUI toast instead).
pub(crate) fn pull_quiet(paths: &Paths, force: bool) -> Result<String> {
    pull_inner(paths, force, false)
}

fn pull_inner(paths: &Paths, force: bool, stderr_warn: bool) -> Result<String> {
    let mut cfg = load_config(paths)?;
    let local_bytes = local_store_bytes(paths)?;
    let local_sha = sha256_hex(&local_bytes);
    let username = cfg.username.clone();
    let password = cfg.password.clone();
    let collection = webdav::namespaced_collection(&cfg.url)?;

    let (remote_bytes, remote_sha) = webdav::block_on(async move {
        let client = DavClient::new(&username, &password)?;
        let store_url = webdav::join_file(&collection, "store.json")?;
        let manifest_url = webdav::join_file(&collection, "manifest.json")?;
        let man_body = client.get(&manifest_url).await?;
        let store_body = client.get(&store_url).await?;
        match (man_body, store_body) {
            (None, None) => anyhow::bail!("remote is empty; nothing to pull"),
            (None, Some(_)) => anyhow::bail!(
                "remote store.json does not match manifest (missing manifest); re-run apmux sync push --force to repair"
            ),
            (Some(_), None) => anyhow::bail!("remote manifest.json present but store.json is missing"),
            (Some(man), Some(store)) => {
                let manifest = parse_manifest(&man)?;
                let got = sha256_hex(&store);
                if got != manifest.sha256 {
                    anyhow::bail!(
                        "remote store.json does not match manifest; re-run apmux sync push --force to repair"
                    );
                }
                if manifest.bytes != 0 && manifest.bytes != store.len() as u64 {
                    anyhow::bail!("remote store.json size does not match manifest");
                }
                Store::from_bytes(&store)?;
                Ok((store, got))
            }
        }
    })?;

    if pull_conflict(&local_sha, &cfg.last_pulled_sha256, &remote_sha) && !force {
        log::info!("webdav.conflict sha={remote_sha}");
        anyhow::bail!("local store has changed since last pull; pass --force to overwrite");
    }

    if local_sha != remote_sha {
        backup::create(paths, None)?;
        fsutil::ensure_dir_0700(&paths.config_dir)?;
        fsutil::atomic_write(&paths.store_file(), &remote_bytes)?;
    }

    let store = Store::from_bytes(&remote_bytes)?;
    cfg.last_pulled_sha256 = remote_sha.clone();
    cfg.last_sync_at = now_local();
    save_config(paths, &cfg)?;
    if stderr_warn {
        switch::reapply_current(paths, &store).context("store pulled but re-apply failed")?;
    } else {
        switch::reapply_current_quiet(paths, &store).context("store pulled but re-apply failed")?;
    }
    Ok(remote_sha)
}

pub(crate) fn status(paths: &Paths) -> Result<String> {
    let cfg = load_config(paths)?;
    let local_sha = sha256_hex(&local_store_bytes(paths)?);
    let username = cfg.username.clone();
    let password = cfg.password.clone();
    let collection = webdav::namespaced_collection(&cfg.url)?;
    let remote = webdav::block_on(async move {
        let client = DavClient::new(&username, &password)?;
        client.ensure_remote_directories(&collection).await?;
        let manifest_url = webdav::join_file(&collection, "manifest.json")?;
        match client.get(&manifest_url).await? {
            None => Ok(RemoteInfo::Missing),
            Some(body) => match parse_manifest(&body) {
                Ok(m) => Ok(RemoteInfo::Sha(m.sha256)),
                Err(_) => Ok(RemoteInfo::Unreadable),
            },
        }
    });
    let (connected, remote_sha, remote_line, conn_err) = match remote {
        Ok(RemoteInfo::Missing) => (true, String::new(), String::new(), String::new()),
        Ok(RemoteInfo::Sha(sha)) => (true, sha.clone(), sha, String::new()),
        Ok(RemoteInfo::Unreadable) => (
            true,
            String::new(),
            "manifest unreadable".to_string(),
            String::new(),
        ),
        Err(e) => (false, String::new(), String::new(), e.to_string()),
    };
    let fork = is_fork(&local_sha, &remote_sha, &cfg.last_pulled_sha256);
    let last_sync = if cfg.last_sync_at.is_empty() {
        "-"
    } else {
        cfg.last_sync_at.as_str()
    };
    let connected_line = if connected {
        "connected: yes".to_string()
    } else {
        format!("connected: no ({conn_err})")
    };
    let out = format!(
        "url: {}\nnamespace: {}\nusername: {}\n{connected_line}\nlocal: {local_sha}\nremote: {remote_line}\nlast_pulled: {}\nlast_pushed: {}\nlast_sync_at: {last_sync}\nfork: {}\n",
        webdav::redact_url(&cfg.url),
        webdav::NAMESPACE,
        cfg.username,
        cfg.last_pulled_sha256,
        cfg.last_pushed_sha256,
        if fork { "yes" } else { "no" },
    );
    debug_assert!(
        cfg.password.is_empty() || !out.contains(&cfg.password),
        "status must never print the password"
    );
    Ok(out)
}

fn load_config(paths: &Paths) -> Result<WebDavConfig> {
    let path = paths.webdav_file();
    if !path.is_file() {
        anyhow::bail!("webdav is not configured; run apmux sync setup");
    }
    let data = fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
    serde_json::from_str(&data).map_err(|e| Error::json(&path, e).into())
}

/// Local WebDAV settings for TUI. Password is never included.
#[derive(Debug, Clone)]
pub(crate) struct LocalSync {
    pub url: String,
    pub username: String,
    pub last_pulled_sha256: String,
    pub last_pushed_sha256: String,
    pub last_sync_at: String,
}

pub(crate) fn local_sync(paths: &Paths) -> Option<LocalSync> {
    let c = load_config(paths).ok()?;
    Some(LocalSync {
        url: c.url,
        username: c.username,
        last_pulled_sha256: c.last_pulled_sha256,
        last_pushed_sha256: c.last_pushed_sha256,
        last_sync_at: c.last_sync_at,
    })
}

pub(crate) fn credentials(paths: &Paths) -> Option<(String, String, String)> {
    load_config(paths)
        .ok()
        .map(|c| (c.url, c.username, c.password))
}

fn save_config(paths: &Paths, cfg: &WebDavConfig) -> Result<()> {
    fsutil::ensure_dir_0700(&paths.config_dir)?;
    let path = paths.webdav_file();
    let mut data = serde_json::to_string_pretty(cfg).context("serialize webdav.json")?;
    if !data.ends_with('\n') {
        data.push('\n');
    }
    fsutil::atomic_write(&path, data.as_bytes())
}

/// Write credentials only. No MKCOL and no hash copy (cc-switch protocol differs).
pub(crate) fn import_config(
    paths: &Paths,
    url: String,
    username: String,
    password: String,
) -> Result<()> {
    if username.is_empty() {
        anyhow::bail!("username must not be empty");
    }
    if password.is_empty() {
        anyhow::bail!("password must not be empty");
    }
    let url = webdav::validate_remote_url(&url)?;
    save_config(
        paths,
        &WebDavConfig {
            url,
            username,
            password,
            last_pulled_sha256: String::new(),
            last_pushed_sha256: String::new(),
            last_sync_at: String::new(),
        },
    )
}

enum RemoteInfo {
    Missing,
    Sha(String),
    Unreadable,
}

fn parse_manifest(body: &[u8]) -> Result<Manifest> {
    let m: Manifest = serde_json::from_slice(body).context("parse remote manifest.json")?;
    if m.format != name::MANIFEST_FORMAT {
        anyhow::bail!("unsupported remote manifest format: {}", m.format);
    }
    if m.version > MANIFEST_VERSION {
        anyhow::bail!("unsupported remote manifest version: {}", m.version);
    }
    Ok(m)
}

pub(crate) fn push_conflict(remote_sha: &str, last_pulled: &str, local_sha: &str) -> bool {
    !remote_sha.is_empty() && remote_sha != last_pulled && remote_sha != local_sha
}

pub(crate) fn pull_conflict(local_sha: &str, last_pulled: &str, remote_sha: &str) -> bool {
    local_sha != last_pulled && remote_sha != local_sha
}

fn is_fork(local_sha: &str, remote_sha: &str, last_pulled: &str) -> bool {
    !remote_sha.is_empty()
        && local_sha != remote_sha
        && local_sha != last_pulled
        && remote_sha != last_pulled
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn local_store_bytes(paths: &Paths) -> Result<Vec<u8>> {
    let file = paths.store_file();
    if file.exists() {
        fs::read(&file).map_err(|e| Error::io(&file, e).into())
    } else {
        serialize_store(&Store::empty())
    }
}

fn serialize_store(store: &Store) -> Result<Vec<u8>> {
    let mut data = serde_json::to_string_pretty(store).context("serialize store.json")?;
    if !data.ends_with('\n') {
        data.push('\n');
    }
    Ok(data.into_bytes())
}

fn device_name() -> String {
    for key in ["HOSTNAME", "COMPUTERNAME"] {
        if let Ok(v) = std::env::var(key) {
            if !v.is_empty() {
                return v;
            }
        }
    }
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn now_local() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{AppId, Provider};
    use crate::webdav::mock::MockServer;

    fn setup_paths() -> (tempfile::TempDir, Paths) {
        let td = tempfile::tempdir().unwrap();
        let paths = Paths::for_test(td.path());
        (td, paths)
    }

    fn sample_store() -> Store {
        let mut store = Store::empty();
        store.providers.insert(
            "packy".into(),
            Provider {
                id: "packy".into(),
                name: "PackyCode".into(),
                app: AppId::Claude,
                base_url: "https://example.com".into(),
                api_key: "sk-test-key-abcd".into(),
                model: None,
                extras: Default::default(),
                ..Provider::blank(AppId::Claude)
            },
        );
        store.current.insert(AppId::Claude, "packy".into());
        store
    }

    fn write_store(paths: &Paths, store: &Store) {
        store.save(paths).unwrap();
    }

    fn cfg_from_disk(paths: &Paths) -> WebDavConfig {
        load_config(paths).unwrap()
    }

    #[cfg(unix)]
    fn unix_mode(path: &std::path::Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn missing_hashes_default_empty() {
        let json = r#"{"url":"http://127.0.0.1/dav","username":"u","password":"p"}"#;
        let cfg: WebDavConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.last_pulled_sha256, "");
        assert_eq!(cfg.last_pushed_sha256, "");
        assert_eq!(cfg.last_sync_at, "");
    }

    #[test]
    fn push_conflict_rules() {
        assert!(!push_conflict("", "", "aaa"));
        assert!(!push_conflict("aaa", "aaa", "bbb"));
        assert!(!push_conflict("aaa", "zzz", "aaa"));
        assert!(push_conflict("aaa", "", "bbb"));
        assert!(push_conflict("aaa", "bbb", "ccc"));
    }

    #[test]
    fn pull_conflict_rules() {
        assert!(!pull_conflict("aaa", "aaa", "bbb"));
        assert!(!pull_conflict("aaa", "", "aaa"));
        assert!(pull_conflict("aaa", "", "bbb"));
        assert!(pull_conflict("aaa", "old", "bbb"));
    }

    #[test]
    fn setup_writes_0600_and_keeps_user_url() {
        let (_td, paths) = setup_paths();
        let srv = MockServer::start();
        let url = srv.collection_url("/dav/my-dir");
        setup(
            &paths,
            url.clone(),
            "user@example.com".into(),
            "app-password".into(),
        )
        .unwrap();
        let cfg = cfg_from_disk(&paths);
        assert_eq!(cfg.url, url);
        assert!(
            !cfg.url.contains("apmux-sync"),
            "stored URL is the WebDAV root, not the namespace: {}",
            cfg.url
        );
        assert_eq!(cfg.last_pulled_sha256, "");
        assert_eq!(cfg.last_pushed_sha256, "");
        #[cfg(unix)]
        {
            assert_eq!(unix_mode(&paths.webdav_file()), 0o600);
        }
        let log = srv.methods();
        assert!(
            log.iter().any(|l| l == "MKCOL /dav/my-dir/apmux-sync"),
            "MKCOL built-in namespace: {log:?}"
        );
    }

    #[test]
    fn setup_rejects_http_and_does_not_write() {
        let (_td, paths) = setup_paths();
        let err = setup(
            &paths,
            "http://example.com/dav".into(),
            "u".into(),
            "p".into(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("localhost"), "{err}");
        assert!(!paths.webdav_file().exists());
    }

    #[test]
    fn push_then_status_hides_password() {
        let (_td, paths) = setup_paths();
        let srv = MockServer::start();
        let url = srv.collection_url("/dav");
        setup(&paths, url, "alice".into(), "s3cret-pass".into()).unwrap();
        write_store(&paths, &sample_store());
        let sha = push(&paths, false).unwrap();
        assert_eq!(sha.len(), 64);
        let cfg = cfg_from_disk(&paths);
        assert_eq!(cfg.last_pushed_sha256, sha);
        assert_eq!(cfg.last_pulled_sha256, sha);
        assert!(!cfg.last_sync_at.is_empty());
        let text = status(&paths).unwrap();
        assert!(text.contains("connected: yes"));
        assert!(text.contains("namespace: apmux-sync"));
        assert!(text.contains(&sha));
        assert!(!text.contains("s3cret-pass"));
        assert!(text.contains("fork: no"));
        let st = srv.state.lock().unwrap();
        assert!(st.files.keys().any(|k| k.ends_with("/store.json")));
        assert!(st.files.keys().any(|k| k.ends_with("/manifest.json")));
        let man: Manifest = serde_json::from_slice(
            st.files
                .iter()
                .find(|(k, _)| k.ends_with("/manifest.json"))
                .unwrap()
                .1,
        )
        .unwrap();
        assert_eq!(man.format, MANIFEST_FORMAT);
        assert_eq!(man.version, 1);
        assert_eq!(man.sha256, sha);
    }

    #[test]
    fn push_conflict_requires_force() {
        let (_td, paths) = setup_paths();
        let srv = MockServer::start();
        let url = srv.collection_url("/dav");
        setup(&paths, url, "u".into(), "p".into()).unwrap();
        write_store(&paths, &sample_store());
        {
            let mut st = srv.state.lock().unwrap();
            st.files.insert(
                "/dav/apmux-sync/manifest.json".into(),
                serde_json::to_vec(&Manifest {
                    format: MANIFEST_FORMAT.into(),
                    version: 1,
                    created_at: "2026-01-01T00:00:00Z".into(),
                    device: "other".into(),
                    bytes: 4,
                    sha256: "deadbeef".into(),
                })
                .unwrap(),
            );
        }
        let err = push(&paths, false).unwrap_err();
        assert!(err.to_string().contains("--force"), "{err}");
        let cfg = cfg_from_disk(&paths);
        assert!(cfg.last_pushed_sha256.is_empty());
        let sha = push(&paths, true).unwrap();
        let cfg = cfg_from_disk(&paths);
        assert_eq!(cfg.last_pushed_sha256, sha);
    }

    #[test]
    fn first_push_empty_remote_does_not_conflict() {
        let (_td, paths) = setup_paths();
        let srv = MockServer::start();
        let url = srv.collection_url("/dav");
        setup(&paths, url, "u".into(), "p".into()).unwrap();
        write_store(&paths, &sample_store());
        push(&paths, false).unwrap();
        let cfg = cfg_from_disk(&paths);
        assert!(!cfg.last_pushed_sha256.is_empty());
    }

    #[test]
    fn pull_backups_then_overwrites_and_force() {
        let (_td, paths) = setup_paths();
        let srv = MockServer::start();
        let url = srv.collection_url("/dav");
        setup(&paths, url, "u".into(), "p".into()).unwrap();
        write_store(&paths, &sample_store());
        let remote_sha = push(&paths, false).unwrap();

        let mut local = sample_store();
        local.providers.get_mut("packy").unwrap().name = "changed".into();
        write_store(&paths, &local);
        let err = pull(&paths, false).unwrap_err();
        assert!(err.to_string().contains("--force"), "{err}");
        let still = Store::load(&paths).unwrap();
        assert_eq!(still.providers["packy"].name, "changed");

        let pulled = pull(&paths, true).unwrap();
        assert_eq!(pulled, remote_sha);
        let restored = Store::load(&paths).unwrap();
        assert_eq!(restored.providers["packy"].name, "PackyCode");
        let backups = backup::list(&paths).unwrap();
        assert!(
            backups.iter().any(|e| e.timestamp),
            "pull must write a timestamp backup first: {backups:?}"
        );
    }

    #[test]
    fn pull_integrity_failure_does_not_touch_store() {
        let (_td, paths) = setup_paths();
        let srv = MockServer::start();
        let url = srv.collection_url("/dav");
        setup(&paths, url, "u".into(), "p".into()).unwrap();
        write_store(&paths, &sample_store());
        let before = fs::read(paths.store_file()).unwrap();
        {
            let mut st = srv.state.lock().unwrap();
            st.files.insert(
                "/dav/apmux-sync/store.json".into(),
                br#"{"version":1,"current":{},"providers":{}}"#.to_vec(),
            );
            st.files.insert(
                "/dav/apmux-sync/manifest.json".into(),
                serde_json::to_vec(&Manifest {
                    format: MANIFEST_FORMAT.into(),
                    version: 1,
                    created_at: "2026-01-01T00:00:00Z".into(),
                    device: "x".into(),
                    bytes: 99,
                    sha256: "abc".into(),
                })
                .unwrap(),
            );
        }
        let err = pull(&paths, true).unwrap_err();
        assert!(err.to_string().contains("does not match manifest"), "{err}");
        assert_eq!(fs::read(paths.store_file()).unwrap(), before);
    }

    #[test]
    fn push_force_overwrites_unreadable_manifest() {
        let (_td, paths) = setup_paths();
        let srv = MockServer::start();
        let url = srv.collection_url("/dav");
        setup(&paths, url, "u".into(), "app-password".into()).unwrap();
        write_store(&paths, &sample_store());
        {
            let mut st = srv.state.lock().unwrap();
            st.files.insert(
                "/dav/apmux-sync/manifest.json".into(),
                b"not-json{{{".to_vec(),
            );
        }
        let err = push(&paths, false).unwrap_err();
        assert!(
            err.to_string().contains("unreadable") && err.to_string().contains("--force"),
            "{err}"
        );
        let cfg = cfg_from_disk(&paths);
        assert!(cfg.last_pushed_sha256.is_empty());
        let text = status(&paths).unwrap();
        assert!(text.contains("connected: yes"), "{text}");
        assert!(text.contains("manifest unreadable"), "{text}");
        let sha = push(&paths, true).unwrap();
        let cfg = cfg_from_disk(&paths);
        assert_eq!(cfg.last_pushed_sha256, sha);
        let st = srv.state.lock().unwrap();
        let man: Manifest =
            serde_json::from_slice(&st.files["/dav/apmux-sync/manifest.json"]).unwrap();
        assert_eq!(man.sha256, sha);
    }

    #[test]
    fn last_pushed_not_updated_if_manifest_put_fails() {
        let (_td, paths) = setup_paths();
        let srv = MockServer::start();
        let url = srv.collection_url("/dav");
        setup(&paths, url, "u".into(), "p".into()).unwrap();
        write_store(&paths, &sample_store());
        {
            let mut st = srv.state.lock().unwrap();
            st.put_fail.insert("/dav/apmux-sync/manifest.json".into());
        }
        let err = push(&paths, false).unwrap_err();
        assert!(err.to_string().contains("PUT"), "{err}");
        let cfg = cfg_from_disk(&paths);
        assert!(
            cfg.last_pushed_sha256.is_empty(),
            "last_pushed_sha256 only updates if store and manifest both succeed"
        );
        let st = srv.state.lock().unwrap();
        assert!(
            st.files.contains_key("/dav/apmux-sync/store.json"),
            "store PUT may succeed before manifest fails"
        );
        assert!(!st.files.contains_key("/dav/apmux-sync/manifest.json"));
    }

    #[test]
    fn store_without_manifest_pull_refuses_push_repairs() {
        let (_td, paths) = setup_paths();
        let srv = MockServer::start();
        let url = srv.collection_url("/dav");
        setup(&paths, url, "u".into(), "p".into()).unwrap();
        write_store(&paths, &sample_store());
        let bytes = fs::read(paths.store_file()).unwrap();
        {
            let mut st = srv.state.lock().unwrap();
            st.files
                .insert("/dav/apmux-sync/store.json".into(), bytes.clone());
            st.files.remove("/dav/apmux-sync/manifest.json");
        }
        let err = pull(&paths, true).unwrap_err();
        assert!(err.to_string().contains("missing manifest"), "{err}");
        push(&paths, false).unwrap();
        let st = srv.state.lock().unwrap();
        assert!(st.files.contains_key("/dav/apmux-sync/manifest.json"));
        let man: Manifest =
            serde_json::from_slice(&st.files["/dav/apmux-sync/manifest.json"]).unwrap();
        assert_eq!(man.sha256, sha256_hex(&bytes));
    }
}
