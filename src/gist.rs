//! GitHub Gist backend for cloud sync: one gist = one remote collection
//! holding the same `store.json` + `manifest.json` pair the WebDAV backend
//! writes. The gist's `description` carries [`crate::name::MANIFEST_FORMAT`]
//! — a storage-format tag, deliberately independent of the product name — so
//! a fresh machine can find its gist by marker. The id, once known, is
//! persisted and is the primary identity (rename-proof).
//!
//! Sync protocol logic (conflict detection, manifest checks, local backups,
//! re-apply) is shared with the WebDAV backend through [`crate::cloud::Remote`].

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Client, Method};
use serde::Deserialize;

use crate::fsutil;
use crate::paths::Paths;

const API_BASE: &str = "https://api.github.com";
const TIMEOUT_SHORT: Duration = Duration::from_secs(30);
const TIMEOUT_LONG: Duration = Duration::from_secs(60);
/// One API page of the user's gists; the marker search walks pages until a
/// short page (or the marker) is found.
const PAGE_SIZE: usize = 100;

/// Local gist credentials + sync state, persisted to `<config>/gist.json`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct GistConfig {
    token: String,
    gist_id: String,
    #[serde(default)]
    last_pulled_sha256: String,
    #[serde(default)]
    last_pushed_sha256: String,
    #[serde(default)]
    last_sync_at: String,
}

fn load_config(paths: &Paths) -> Result<GistConfig> {
    let bytes = std::fs::read(paths.gist_file()).context("read gist.json")?;
    serde_json::from_slice(&bytes).context("parse gist.json")
}

fn save_config(paths: &Paths, cfg: &GistConfig) -> Result<()> {
    fsutil::ensure_dir_0700(&paths.config_dir)?;
    let mut data = serde_json::to_string_pretty(cfg).context("serialize gist.json")?;
    if !data.ends_with('\n') {
        data.push('\n');
    }
    fsutil::atomic_write(&paths.gist_file(), data.as_bytes())
}

#[derive(Debug, Deserialize)]
struct ApiFile {
    #[serde(default)]
    truncated: bool,
    content: Option<String>,
    raw_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiGist {
    id: String,
    files: HashMap<String, ApiFile>,
}

#[derive(Debug, Deserialize)]
struct ApiGistSummary {
    id: String,
    description: Option<String>,
}

pub(crate) struct GistClient {
    http: Client,
    token: String,
    base: String,
}

impl GistClient {
    pub(crate) fn new(token: &str) -> Result<Self> {
        Self::with_base(token, API_BASE)
    }

    /// Alternate API base; tests point this at the mock server.
    fn with_base(token: &str, base: &str) -> Result<Self> {
        let http = Client::builder()
            .user_agent(concat!(env!("CARGO_PKG_NAME"), "-gist-sync/1.0"))
            .build()
            .context("http client")?;
        Ok(Self {
            http,
            token: token.to_string(),
            base: base.to_string(),
        })
    }

    async fn send(
        &self,
        method: Method,
        url: &str,
        body: Option<Vec<u8>>,
        long: bool,
    ) -> Result<(u16, Vec<u8>)> {
        let timeout = if long { TIMEOUT_LONG } else { TIMEOUT_SHORT };
        let mut req = self
            .http
            .request(method, url)
            .bearer_auth(&self.token)
            .timeout(timeout);
        if let Some(body) = body {
            req = req.body(body);
        }
        let resp = req.send().await?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.unwrap_or_default().to_vec();
        Ok((status, bytes))
    }

    /// First gist whose description equals `marker`, newest first.
    pub(crate) async fn find_by_description(&self, marker: &str) -> Result<Option<String>> {
        let mut page = 1;
        loop {
            let url = format!("{}/gists?per_page={PAGE_SIZE}&page={page}", self.base);
            let (status, body) = self.send(Method::GET, &url, None, false).await?;
            match status {
                200 => {}
                401 | 403 => anyhow::bail!("gist auth failed (token needs Gists read/write)"),
                other => anyhow::bail!("list gists failed: HTTP {other}"),
            }
            let gists: Vec<ApiGistSummary> =
                serde_json::from_slice(&body).context("parse gist list")?;
            if let Some(g) = gists
                .iter()
                .find(|g| g.description.as_deref() == Some(marker))
            {
                return Ok(Some(g.id.clone()));
            }
            if gists.len() < PAGE_SIZE {
                return Ok(None);
            }
            page += 1;
        }
    }

    /// Create a secret gist; returns its id.
    pub(crate) async fn create(
        &self,
        description: &str,
        files: &[(&str, String)],
    ) -> Result<String> {
        let files = files
            .iter()
            .map(|(name, content)| (name.to_string(), serde_json::json!({"content": content})))
            .collect::<serde_json::Map<_, _>>();
        let payload = serde_json::json!({
            "description": description,
            "public": false,
            "files": files,
        });
        let (status, body) = self
            .send(
                Method::POST,
                &format!("{}/gists", self.base),
                Some(payload.to_string().into_bytes()),
                true,
            )
            .await?;
        match status {
            201 => {}
            401 | 403 => anyhow::bail!("gist auth failed (token needs Gists read/write)"),
            other => anyhow::bail!("create gist failed: HTTP {other}"),
        }
        let gist: ApiGist = serde_json::from_slice(&body).context("parse create gist response")?;
        Ok(gist.id)
    }

    pub(crate) async fn exists(&self, gist_id: &str) -> Result<bool> {
        let (status, _) = self
            .send(
                Method::GET,
                &format!("{}/gists/{gist_id}", self.base),
                None,
                false,
            )
            .await?;
        match status {
            200 => Ok(true),
            404 => Ok(false),
            401 | 403 => anyhow::bail!("gist auth failed (token needs Gists read/write)"),
            other => anyhow::bail!("GET gist {gist_id} failed: HTTP {other}"),
        }
    }

    /// Full content of one file; falls back to the raw URL when the API
    /// omits or truncates embedded content.
    pub(crate) async fn get_file(&self, gist_id: &str, name: &str) -> Result<Option<Vec<u8>>> {
        let (status, body) = self
            .send(
                Method::GET,
                &format!("{}/gists/{gist_id}", self.base),
                None,
                true,
            )
            .await?;
        match status {
            200 => {}
            // The gist itself is gone; the protocol reports "remote empty".
            404 => return Ok(None),
            401 | 403 => anyhow::bail!("gist auth failed (token needs Gists read/write)"),
            other => anyhow::bail!("GET gist {gist_id} failed: HTTP {other}"),
        }
        let gist: ApiGist = serde_json::from_slice(&body).context("parse gist response")?;
        let Some(file) = gist.files.get(name) else {
            return Ok(None);
        };
        if !file.truncated {
            if let Some(content) = &file.content {
                return Ok(Some(content.clone().into_bytes()));
            }
        }
        // Truncated or content-less: fetch the raw file.
        let raw = file
            .raw_url
            .as_deref()
            .context("gist file has no raw_url")?;
        let (status, raw_body) = self.send(Method::GET, raw, None, true).await?;
        match status {
            200 => Ok(Some(raw_body)),
            404 => Ok(None),
            401 | 403 => anyhow::bail!("gist auth failed (token needs Gists read/write)"),
            other => anyhow::bail!("GET {raw} failed: HTTP {other}"),
        }
    }

    /// Add or update named files in the gist.
    pub(crate) async fn update(&self, gist_id: &str, files: &[(&str, String)]) -> Result<()> {
        let files = files
            .iter()
            .map(|(name, content)| (name.to_string(), serde_json::json!({"content": content})))
            .collect::<serde_json::Map<_, _>>();
        let payload = serde_json::json!({ "files": files });
        let (status, _) = self
            .send(
                Method::PATCH,
                &format!("{}/gists/{gist_id}", self.base),
                Some(payload.to_string().into_bytes()),
                true,
            )
            .await?;
        match status {
            200 => Ok(()),
            404 => anyhow::bail!(
                "gist {gist_id} not found; re-run `{} sync gist setup`",
                crate::name::NAME
            ),
            401 | 403 => anyhow::bail!("gist auth failed (token needs Gists read/write)"),
            other => anyhow::bail!("PATCH gist {gist_id} failed: HTTP {other}"),
        }
    }
}

/// Accepts a bare gist id, `owner/id`, or a gist URL; returns the id.
fn parse_gist_id(spec: &str) -> Result<String> {
    let s = spec.trim();
    let last = s.rsplit('/').next().unwrap_or(s);
    let is_hex =
        !last.is_empty() && last.len() >= 20 && last.chars().all(|c| c.is_ascii_hexdigit());
    if is_hex {
        Ok(last.to_ascii_lowercase())
    } else {
        anyhow::bail!("invalid gist id or URL: {spec}")
    }
}

/// Configure gist sync. Creates a new secret gist seeded with the current
/// local store (so the remote starts identical to local), or reuses an
/// existing one found by the format marker in its description — unless
/// `--gist` pins a specific gist. Returns the gist id in use.
pub(crate) fn setup(paths: &Paths, token: String, gist: Option<String>) -> Result<String> {
    if token.is_empty() {
        anyhow::bail!("token must not be empty");
    }
    let client = GistClient::new(&token)?;
    setup_client(paths, &client, token, gist)
}

fn setup_client(
    paths: &Paths,
    client: &GistClient,
    token: String,
    gist: Option<String>,
) -> Result<String> {
    let marker = crate::name::MANIFEST_FORMAT;

    let mut cfg = load_config(paths).unwrap_or_default();
    let (id, seeded_sha) = if let Some(spec) = gist {
        let id = parse_gist_id(&spec)?;
        if !crate::webdav::block_on(async { client.exists(&id).await })? {
            anyhow::bail!(
                "gist {id} not found; check the id, or drop --gist to create or find one"
            );
        }
        (id, None)
    } else {
        match crate::webdav::block_on(async { client.find_by_description(marker).await })? {
            Some(id) => (id, None),
            None => {
                let (id, sha) = create_and_seed(paths, client, marker)?;
                (id, Some(sha))
            }
        }
    };

    if let Some(sha) = seeded_sha {
        cfg.last_pulled_sha256 = sha.clone();
        cfg.last_pushed_sha256 = sha;
        cfg.last_sync_at = crate::cloud::now_local();
    } else if cfg.gist_id != id {
        // Different gist than before: sync state is meaningless.
        cfg.last_pulled_sha256.clear();
        cfg.last_pushed_sha256.clear();
        cfg.last_sync_at.clear();
    }
    cfg.gist_id = id.clone();
    cfg.token = token;
    save_config(paths, &cfg)?;
    Ok(id)
}

/// Fresh gist seeded with the current local store + manifest, so the remote
/// starts identical to local. Returns (gist id, local sha).
fn create_and_seed(paths: &Paths, client: &GistClient, marker: &str) -> Result<(String, String)> {
    let local_bytes = crate::cloud::local_store_bytes(paths)?;
    let local_sha = crate::cloud::sha256_hex(&local_bytes);
    let manifest = crate::cloud::build_manifest(&local_bytes, &local_sha)?;
    let store = String::from_utf8(local_bytes).context("store.json is not UTF-8")?;
    let id = crate::webdav::block_on(async {
        client
            .create(
                marker,
                &[("manifest.json", manifest), ("store.json", store)],
            )
            .await
    })?;
    log::info!("gist.created id={id} sha={local_sha}");
    Ok((id, local_sha))
}

/// The gist backend behind [`crate::cloud::Remote`].
pub(crate) struct GistRemote {
    client: GistClient,
    gist_id: String,
}

impl crate::cloud::Remote for GistRemote {
    fn ensure_ready(&self) -> Result<()> {
        // A single remote container; readiness = the gist still exists.
        if !crate::webdav::block_on(async { self.client.exists(&self.gist_id).await })? {
            anyhow::bail!(
                "gist {} not found; re-run `{} sync gist setup`",
                self.gist_id,
                crate::name::NAME
            );
        }
        Ok(())
    }

    fn get(&self, name: &str) -> Result<Option<Vec<u8>>> {
        crate::webdav::block_on(async { self.client.get_file(&self.gist_id, name).await })
    }

    fn put(&self, name: &str, body: &[u8]) -> Result<()> {
        let content = std::str::from_utf8(body).context("sync files must be UTF-8 JSON")?;
        crate::webdav::block_on(async {
            self.client
                .update(&self.gist_id, &[(name, content.to_string())])
                .await
        })
    }
}

fn remote_from(cfg: &GistConfig) -> Result<GistRemote> {
    if cfg.gist_id.is_empty() {
        anyhow::bail!(
            "gist sync is not set up; run `{} sync gist setup`",
            crate::name::NAME
        );
    }
    Ok(GistRemote {
        client: GistClient::new(&cfg.token)?,
        gist_id: cfg.gist_id.clone(),
    })
}

fn sync_state(cfg: &mut GistConfig) -> crate::cloud::SyncState {
    crate::cloud::SyncState {
        last_pulled_sha256: std::mem::take(&mut cfg.last_pulled_sha256),
        last_pushed_sha256: std::mem::take(&mut cfg.last_pushed_sha256),
        last_sync_at: std::mem::take(&mut cfg.last_sync_at),
    }
}

fn writeback(cfg: &mut GistConfig, state: crate::cloud::SyncState) {
    cfg.last_pulled_sha256 = state.last_pulled_sha256;
    cfg.last_pushed_sha256 = state.last_pushed_sha256;
    cfg.last_sync_at = state.last_sync_at;
}

pub(crate) fn push(paths: &Paths, force: bool) -> Result<String> {
    let mut cfg = load_config(paths)?;
    let remote = remote_from(&cfg)?;
    push_remote(paths, &mut cfg, &remote, force)
}

fn push_remote(
    paths: &Paths,
    cfg: &mut GistConfig,
    remote: &GistRemote,
    force: bool,
) -> Result<String> {
    let mut state = sync_state(cfg);
    let sha = crate::cloud::push_with(paths, remote, &mut state, force, "gist")?;
    writeback(cfg, state);
    save_config(paths, cfg)?;
    Ok(sha)
}

pub(crate) fn pull(paths: &Paths, force: bool) -> Result<String> {
    let mut cfg = load_config(paths)?;
    let remote = remote_from(&cfg)?;
    pull_remote(paths, &mut cfg, &remote, force)
}

fn pull_remote(
    paths: &Paths,
    cfg: &mut GistConfig,
    remote: &GistRemote,
    force: bool,
) -> Result<String> {
    let mut state = sync_state(cfg);
    let sha = crate::cloud::pull_with(paths, remote, &mut state, force, true, "gist")?;
    writeback(cfg, state);
    save_config(paths, cfg)?;
    Ok(sha)
}

pub(crate) fn status(paths: &Paths) -> Result<String> {
    let cfg = match load_config(paths) {
        Ok(cfg) => cfg,
        Err(_) => anyhow::bail!(
            "gist sync is not set up; run `{} sync gist setup`",
            crate::name::NAME
        ),
    };
    let remote = remote_from(&cfg)?;
    status_remote(paths, &cfg, &remote)
}

fn status_remote(paths: &Paths, cfg: &GistConfig, remote: &GistRemote) -> Result<String> {
    let local_sha = crate::cloud::sha256_hex(&crate::cloud::local_store_bytes(paths)?);
    let (connected, remote_sha, remote_line, conn_err) = match crate::cloud::remote_info(remote) {
        Ok(crate::cloud::RemoteInfo::Missing) => {
            (true, String::new(), String::new(), String::new())
        }
        Ok(crate::cloud::RemoteInfo::Sha(sha)) => (true, sha.clone(), sha, String::new()),
        Ok(crate::cloud::RemoteInfo::Unreadable) => (
            true,
            String::new(),
            "manifest unreadable".to_string(),
            String::new(),
        ),
        Err(e) => (false, String::new(), String::new(), e.to_string()),
    };
    let fork = crate::cloud::is_fork(&local_sha, &remote_sha, &cfg.last_pulled_sha256);
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
    // The token is never printed.
    Ok(format!(
        "gist: {}\nurl: https://gist.github.com/{}\n{connected_line}\nlocal: {local_sha}\nremote: {remote_line}\nlast_pulled: {}\nlast_pushed: {}\nlast_sync_at: {last_sync}\nfork: {}\n",
        cfg.gist_id,
        cfg.gist_id,
        cfg.last_pulled_sha256,
        cfg.last_pushed_sha256,
        if fork { "yes" } else { "no" },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;
    use mock::{MockGist, MockGistFile, MockServer};

    fn provider_at(name: &str) -> crate::store::Provider {
        crate::store::Provider {
            id: format!("codex-{name}"),
            name: name.into(),
            app: crate::store::AppId::Codex,
            base_url: "https://x.example.com/v1".into(),
            api_key: "sk-test".into(),
            ..crate::store::Provider::blank(crate::store::AppId::Codex)
        }
    }

    /// A valid serialized store whose content varies with `marker`.
    fn store_bytes(marker: &str) -> Vec<u8> {
        let mut store = crate::store::Store::empty();
        store
            .providers
            .insert(format!("codex-{marker}"), provider_at(marker));
        serde_json::to_vec_pretty(&store).expect("serialize store")
    }

    fn setup_paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::for_test(dir.path());
        fsutil::ensure_dir_0700(&paths.config_dir).expect("config dir");
        fsutil::atomic_write(&paths.store_file(), &store_bytes("seed")).expect("write store");
        (dir, paths)
    }

    fn client_for(srv: &MockServer) -> GistClient {
        GistClient::with_base("ghp_test", &srv.base_url()).expect("test client")
    }

    fn remote_for(srv: &MockServer, cfg: &GistConfig) -> GistRemote {
        GistRemote {
            client: client_for(srv),
            gist_id: cfg.gist_id.clone(),
        }
    }

    #[test]
    fn setup_creates_secret_gist_seeded_with_store() {
        let (_dir, paths) = setup_paths();
        let srv = MockServer::start();
        let token = "ghp_test".to_string();
        let client = client_for(&srv);
        let id = setup_client(&paths, &client, token.clone(), None).expect("setup");

        let gists = srv.gists();
        assert_eq!(gists.len(), 1);
        let g = gists.get(&id).expect("created gist");
        assert_eq!(g.description, crate::name::MANIFEST_FORMAT);
        assert!(g.files.contains_key("store.json"));
        assert!(g.files.contains_key("manifest.json"));

        // Sync state recorded: remote equals local.
        let cfg = load_config(&paths).unwrap();
        assert_eq!(cfg.gist_id, id);
        assert_eq!(cfg.token, token);
        assert_eq!(cfg.last_pushed_sha256, cfg.last_pulled_sha256);
        assert!(!cfg.last_pushed_sha256.is_empty());

        // Status shows the gist and never the token.
        let remote = remote_for(&srv, &cfg);
        let out = status_remote(&paths, &cfg, &remote).unwrap();
        assert!(out.contains(&id));
        assert!(!out.contains(&token));
        assert!(out.contains("fork: no"));
    }

    #[test]
    fn setup_finds_existing_gist_by_marker() {
        let (_dir, paths) = setup_paths();
        let srv = MockServer::start();
        srv.insert(
            "existing-id",
            MockGist {
                description: crate::name::MANIFEST_FORMAT.to_string(),
                files: HashMap::new(),
            },
        );
        let client = client_for(&srv);
        let id = setup_client(&paths, &client, "ghp_test".into(), None).expect("setup");
        assert_eq!(id, "existing-id");
        assert_eq!(srv.gists().len(), 1, "no second gist may be created");
        // Reusing a foreign gist resets sync state.
        let cfg = load_config(&paths).unwrap();
        assert!(cfg.last_pushed_sha256.is_empty());
    }

    #[test]
    fn parse_gist_id_forms() {
        let id32 = "abc123def456abc123def456abc123de";
        assert_eq!(parse_gist_id(id32).unwrap(), id32);
        assert_eq!(
            parse_gist_id("owner/ABC123DEF456ABC123DEF456ABC123DE").unwrap(),
            id32
        );
        assert_eq!(
            parse_gist_id(&format!("https://gist.github.com/owner/{id32}")).unwrap(),
            id32
        );
        assert!(parse_gist_id("not-a-gist").is_err());
        assert!(parse_gist_id("https://gist.github.com/").is_err());
    }

    #[test]
    fn push_pull_roundtrip_and_conflict() {
        let (_dir, paths) = setup_paths();
        let srv = MockServer::start();
        let client = client_for(&srv);
        setup_client(&paths, &client, "ghp_test".into(), None).expect("setup");

        // Local change → push → remote updated.
        let before = srv.remote_store().unwrap();
        fsutil::atomic_write(&paths.store_file(), &store_bytes("one")).expect("write");
        let mut cfg = load_config(&paths).unwrap();
        let remote = remote_for(&srv, &cfg);
        push_remote(&paths, &mut cfg, &remote, false).expect("push");
        let after = srv.remote_store().unwrap();
        assert_ne!(before, after);

        // Simulate another machine pushing: remote manifest sha moves on.
        srv.set_remote_manifest_sha("deadbeef");
        fsutil::atomic_write(&paths.store_file(), &store_bytes("two")).expect("write");
        let remote = remote_for(&srv, &cfg);
        let err = push_remote(&paths, &mut cfg, &remote, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("remote store has changed"), "{err}");
        let remote = remote_for(&srv, &cfg);
        let sha = push_remote(&paths, &mut cfg, &remote, true).expect("force push");

        // Local change since last sync → pull conflicts without --force.
        srv.set_remote_manifest_sha(&sha);
        fsutil::atomic_write(&paths.store_file(), &store_bytes("three")).expect("write");
        let remote = remote_for(&srv, &cfg);
        let err = pull_remote(&paths, &mut cfg, &remote, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("local store has changed"), "{err}");
        let remote = remote_for(&srv, &cfg);
        pull_remote(&paths, &mut cfg, &remote, true).expect("force pull");
        let local = std::fs::read_to_string(paths.store_file()).unwrap();
        assert_eq!(local, srv.remote_store().unwrap());
    }

    #[test]
    fn missing_gist_points_back_to_setup() {
        let (_dir, paths) = setup_paths();
        let srv = MockServer::start();
        let client = client_for(&srv);
        setup_client(&paths, &client, "ghp_test".into(), None).expect("setup");
        srv.clear();
        let mut cfg = load_config(&paths).unwrap();
        let remote = remote_for(&srv, &cfg);
        let err = push_remote(&paths, &mut cfg, &remote, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("sync gist setup"), "{err}");
    }

    #[test]
    fn auth_failure_is_clear() {
        let (_dir, paths) = setup_paths();
        let srv = MockServer::start();
        srv.set_auth_ok(false);
        let client = client_for(&srv);
        let err = setup_client(&paths, &client, "ghp_test".into(), None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("gist auth failed"), "{err}");
    }

    #[test]
    fn truncated_file_falls_back_to_raw() {
        let (_dir, paths) = setup_paths();
        let srv = MockServer::start();
        let client = client_for(&srv);
        let id = setup_client(&paths, &client, "ghp_test".into(), None).expect("setup");
        // Re-file the manifest as API-truncated: content omitted, raw intact.
        let g = srv.gists().remove(&id).unwrap();
        let raw_manifest = g.files.get("manifest.json").unwrap().raw.clone();
        let mut files = g.files.clone();
        files.insert(
            "manifest.json".to_string(),
            MockGistFile {
                content: None,
                truncated: true,
                raw: raw_manifest,
            },
        );
        srv.insert(
            &id,
            MockGist {
                description: g.description,
                files,
            },
        );
        // Pull must still work via the raw fallback.
        let mut cfg = load_config(&paths).unwrap();
        let remote = remote_for(&srv, &cfg);
        pull_remote(&paths, &mut cfg, &remote, true).expect("pull via raw fallback");
    }
}

#[cfg(test)]
pub(crate) mod mock {
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    #[derive(Debug, Clone)]
    pub(crate) struct MockGistFile {
        pub content: Option<String>,
        pub truncated: bool,
        pub raw: String,
    }

    #[derive(Debug, Clone)]
    pub(crate) struct MockGist {
        pub description: String,
        pub files: HashMap<String, MockGistFile>,
    }

    #[derive(Default)]
    pub(crate) struct MockState {
        pub gists: HashMap<String, MockGist>,
        pub counter: u64,
        pub auth_ok: bool,
        pub log: Vec<String>,
    }

    pub(crate) struct MockServer {
        pub addr: SocketAddr,
        pub state: Arc<Mutex<MockState>>,
    }

    impl MockServer {
        pub(crate) fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock gist");
            let addr = listener.local_addr().expect("local addr");
            let state = Arc::new(Mutex::new(MockState {
                auth_ok: true,
                ..MockState::default()
            }));
            let st = state.clone();
            thread::spawn(move || serve(listener, st, addr));
            Self { addr, state }
        }

        pub(crate) fn base_url(&self) -> String {
            format!("http://{}", self.addr)
        }

        pub(crate) fn gists(&self) -> HashMap<String, MockGist> {
            self.state.lock().expect("mock state").gists.clone()
        }

        pub(crate) fn insert(&self, id: &str, gist: MockGist) {
            self.state
                .lock()
                .expect("mock state")
                .gists
                .insert(id.to_string(), gist);
        }

        pub(crate) fn clear(&self) {
            self.state.lock().expect("mock state").gists.clear();
        }

        pub(crate) fn set_auth_ok(&self, ok: bool) {
            self.state.lock().expect("mock state").auth_ok = ok;
        }

        /// Raw `store.json` content of the first (only) gist.
        pub(crate) fn remote_store(&self) -> Option<String> {
            self.gists()
                .values()
                .next()
                .and_then(|g| g.files.get("store.json"))
                .map(|f| f.raw.clone())
        }

        /// Move the remote manifest sha without touching the store.
        pub(crate) fn set_remote_manifest_sha(&self, sha: &str) {
            let mut st = self.state.lock().expect("mock state");
            for g in st.gists.values_mut() {
                if let Some(f) = g.files.get_mut("manifest.json") {
                    let mut m: serde_json::Value =
                        serde_json::from_str(&f.raw).expect("mock manifest json");
                    m["sha256"] = serde_json::Value::String(sha.to_string());
                    let raw = m.to_string();
                    f.raw = raw.clone();
                    f.content = Some(raw);
                    f.truncated = false;
                }
            }
        }
    }

    fn serve(listener: TcpListener, state: Arc<Mutex<MockState>>, addr: SocketAddr) {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let st = state.clone();
            thread::spawn(move || {
                let _ = handle_conn(stream, st, addr);
            });
        }
    }

    fn gist_json(addr: SocketAddr, id: &str, g: &MockGist) -> serde_json::Value {
        let files = g
            .files
            .iter()
            .map(|(name, f)| {
                (
                    name.clone(),
                    serde_json::json!({
                        "filename": name,
                        "content": f.content,
                        "truncated": f.truncated,
                        "raw_url": format!("http://{addr}/raw/{id}/{name}"),
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        serde_json::json!({
            "id": id,
            "description": g.description,
            "owner": {"login": "mock"},
            "files": files,
        })
    }

    fn respond(
        addr: SocketAddr,
        st: &mut MockState,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> (u16, Vec<u8>) {
        if !st.auth_ok {
            return (401, Vec::new());
        }
        let path = path.split('?').next().unwrap_or(path);
        let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        match (method, segs.as_slice()) {
            ("GET", ["gists"]) => {
                let list: Vec<serde_json::Value> = st
                    .gists
                    .iter()
                    .map(|(id, g)| serde_json::json!({"id": id, "description": g.description}))
                    .collect();
                (200, serde_json::to_vec(&list).unwrap())
            }
            ("POST", ["gists"]) => {
                let req: serde_json::Value = serde_json::from_slice(body).unwrap();
                let id = format!("{:032x}", st.counter);
                st.counter += 1;
                let mut files = HashMap::new();
                for (name, v) in req["files"].as_object().unwrap() {
                    let content = v["content"].as_str().unwrap_or("").to_string();
                    files.insert(
                        name.clone(),
                        MockGistFile {
                            content: Some(content.clone()),
                            truncated: false,
                            raw: content,
                        },
                    );
                }
                let g = MockGist {
                    description: req["description"].as_str().unwrap_or("").to_string(),
                    files,
                };
                let body = gist_json(addr, &id, &g).to_string().into_bytes();
                st.gists.insert(id.clone(), g);
                (201, body)
            }
            ("GET", ["gists", id]) => match st.gists.get(*id) {
                Some(g) => (200, gist_json(addr, id, g).to_string().into_bytes()),
                None => (404, Vec::new()),
            },
            ("PATCH", ["gists", id]) => match st.gists.get_mut(*id) {
                Some(g) => {
                    let req: serde_json::Value = serde_json::from_slice(body).unwrap();
                    for (name, v) in req["files"].as_object().unwrap() {
                        let content = v["content"].as_str().unwrap_or("").to_string();
                        g.files.insert(
                            name.clone(),
                            MockGistFile {
                                content: Some(content.clone()),
                                truncated: false,
                                raw: content,
                            },
                        );
                    }
                    (200, gist_json(addr, id, g).to_string().into_bytes())
                }
                None => (404, Vec::new()),
            },
            ("GET", ["raw", id, name]) => {
                match st.gists.get(*id).and_then(|g| g.files.get(*name)) {
                    Some(f) => (200, f.raw.clone().into_bytes()),
                    None => (404, Vec::new()),
                }
            }
            _ => (405, Vec::new()),
        }
    }

    fn handle_conn(
        mut stream: TcpStream,
        state: Arc<Mutex<MockState>>,
        addr: SocketAddr,
    ) -> std::io::Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(3))).ok();
        while let Ok((method, path, req_body)) = read_request(&mut stream) {
            if method.is_empty() {
                break;
            }
            let (code, body) = {
                let mut st = state.lock().expect("mock state");
                st.log.push(format!("{method} {path}"));
                respond(addr, &mut st, &method, &path, &req_body)
            };
            write_response(&mut stream, code, &body)?;
        }
        Ok(())
    }

    fn read_until_double_crlf(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            stream.read_exact(&mut byte)?;
            buf.push(byte[0]);
            if buf.ends_with(b"\r\n\r\n") {
                return Ok(buf);
            }
        }
    }

    fn read_request(stream: &mut TcpStream) -> std::io::Result<(String, String, Vec<u8>)> {
        let header_bytes = read_until_double_crlf(stream)?;
        if header_bytes.is_empty() {
            return Ok((String::new(), String::new(), Vec::new()));
        }
        let header = String::from_utf8_lossy(&header_bytes);
        let mut lines = header.split("\r\n");
        let start = lines.next().unwrap_or("");
        let mut sp = start.split_whitespace();
        let method = sp.next().unwrap_or("").to_string();
        let path = sp.next().unwrap_or("").to_string();
        let mut content_length = 0usize;
        for line in lines {
            let lower = line.to_ascii_lowercase();
            if let Some(v) = lower.strip_prefix("content-length:") {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            stream.read_exact(&mut body)?;
        }
        Ok((method, path, body))
    }

    fn write_response(stream: &mut TcpStream, code: u16, body: &[u8]) -> std::io::Result<()> {
        let reason = match code {
            200 => "OK",
            201 => "Created",
            401 => "Unauthorized",
            404 => "Not Found",
            405 => "Method Not Allowed",
            _ => "?",
        };
        write!(
            stream,
            "HTTP/1.1 {code} {reason}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
            body.len()
        )?;
        stream.write_all(body)?;
        stream.flush()
    }
}
