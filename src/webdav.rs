use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::{Client, Method, StatusCode, Url};

const TIMEOUT_SHORT: Duration = Duration::from_secs(30);
const TIMEOUT_LONG: Duration = Duration::from_secs(60);

/// Built-in remote folder under the user-supplied WebDAV root. Not user-editable.
pub(crate) const NAMESPACE: &str = crate::name::SYNC_NAMESPACE;

pub(crate) fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .context("tokio runtime")
}

pub(crate) fn block_on<T>(fut: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    runtime()?.block_on(fut)
}

/// WebDAV root as the user typed it (trimmed). Does not append [`NAMESPACE`].
pub(crate) fn validate_remote_url(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        anyhow::bail!("url must not be empty");
    }
    let url = Url::parse(raw).map_err(|_| anyhow!("invalid url: {raw}"))?;
    match url.scheme() {
        "https" => {}
        "http" => {
            if !is_localhost_host(url.host_str()) {
                anyhow::bail!("http:// is only allowed for localhost; use https://");
            }
        }
        _ => anyhow::bail!("url must be http or https"),
    }
    if url.host_str().is_none() {
        anyhow::bail!("invalid url: {raw}");
    }
    Ok(raw.to_string())
}

fn host_key(host: Option<&str>) -> String {
    host.unwrap_or("")
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

fn is_localhost_host(host: Option<&str>) -> bool {
    matches!(host_key(host).as_str(), "localhost" | "127.0.0.1" | "::1")
}

pub(crate) fn join_file(collection: &str, name: &str) -> Result<String> {
    let base = if collection.ends_with('/') {
        collection.to_string()
    } else {
        format!("{collection}/")
    };
    Url::parse(&base)
        .map_err(|e| anyhow!("invalid url: {e}"))?
        .join(name)
        .map(|u| u.to_string())
        .map_err(|e| anyhow!("invalid url: {e}"))
}

/// `{base}/{NAMESPACE}` — where store.json and manifest.json live.
pub(crate) fn namespaced_collection(base: &str) -> Result<String> {
    join_dir(base, NAMESPACE)
}

/// Append directory segments without replacing the last path component.
/// `https://host/dav` + `aimux-sync` → `https://host/dav/aimux-sync`.
pub(crate) fn join_dir(base: &str, extra: &str) -> Result<String> {
    let base = base.trim();
    let extra = extra.trim().trim_matches('/');
    if extra.is_empty() {
        return Ok(base.to_string());
    }
    let mut url = Url::parse(base).map_err(|e| anyhow!("invalid url: {e}"))?;
    {
        let mut segs = url
            .path_segments_mut()
            .map_err(|_| anyhow!("invalid url: {base}"))?;
        segs.pop_if_empty();
        for seg in extra.split('/') {
            let seg = seg.trim();
            if seg.is_empty() || seg == "." {
                continue;
            }
            if seg == ".." {
                anyhow::bail!("path must not contain '..'");
            }
            segs.push(seg);
        }
    }
    Ok(url.to_string())
}

pub(crate) fn collection_urls(collection: &str) -> Result<Vec<String>> {
    let url = Url::parse(collection).map_err(|e| anyhow!("invalid url: {e}"))?;
    let mut acc = String::new();
    let mut out = Vec::new();
    for seg in url.path_segments().into_iter().flatten() {
        if seg.is_empty() {
            continue;
        }
        acc.push('/');
        acc.push_str(seg);
        let mut u = url.clone();
        u.set_path(&acc);
        u.set_query(None);
        u.set_fragment(None);
        out.push(u.to_string());
    }
    Ok(out)
}

pub(crate) fn redact_url(s: &str) -> String {
    match Url::parse(s) {
        Ok(mut u) => {
            if !u.username().is_empty() {
                let _ = u.set_username("***");
            }
            if u.password().is_some() {
                let _ = u.set_password(Some("***"));
            }
            u.to_string()
        }
        Err(_) => s.to_string(),
    }
}

fn dav_method(name: &[u8]) -> Method {
    Method::from_bytes(name).unwrap_or(Method::GET)
}

pub(crate) struct DavClient {
    http: Client,
    username: String,
    password: String,
}

impl DavClient {
    pub(crate) fn new(username: &str, password: &str) -> Result<Self> {
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .http1_only()
            .build()
            .context("http client")?;
        Ok(Self {
            http,
            username: username.to_string(),
            password: password.to_string(),
        })
    }

    async fn send(
        &self,
        method: Method,
        url: &str,
        body: Option<Vec<u8>>,
        extra: &[(&str, &str)],
        long: bool,
    ) -> Result<(StatusCode, Vec<u8>)> {
        let timeout = if long { TIMEOUT_LONG } else { TIMEOUT_SHORT };
        let mut req = self
            .http
            .request(method, url)
            .basic_auth(&self.username, Some(&self.password))
            .timeout(timeout);
        for (k, v) in extra {
            req = req.header(*k, *v);
        }
        if let Some(body) = body {
            req = req.body(body);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await.unwrap_or_default().to_vec();
        Ok((status, bytes))
    }

    pub(crate) async fn propfind_exists(&self, url: &str) -> Result<bool> {
        let method = dav_method(b"PROPFIND");
        let body = b"<?xml version=\"1.0\" encoding=\"utf-8\"?><propfind xmlns=\"DAV:\"><prop><resourcetype/></prop></propfind>".to_vec();
        let (status, _) = self
            .send(
                method,
                url,
                Some(body),
                &[("Depth", "0"), ("Content-Type", "application/xml")],
                false,
            )
            .await?;
        match status.as_u16() {
            200 | 207 | 301 | 302 | 308 => Ok(true),
            404 => Ok(false),
            401 | 403 => anyhow::bail!("webdav auth failed"),
            other => anyhow::bail!("PROPFIND {} failed: HTTP {other}", redact_url(url)),
        }
    }

    pub(crate) async fn mkcol(&self, url: &str) -> Result<MkcolOutcome> {
        let method = dav_method(b"MKCOL");
        let (status, _) = self.send(method, url, None, &[], false).await?;
        match status.as_u16() {
            200 | 201 | 204 => Ok(MkcolOutcome::Created),
            405 | 409 => Ok(MkcolOutcome::MaybeExists),
            401 | 403 => anyhow::bail!("webdav auth failed"),
            other => anyhow::bail!("MKCOL {} failed: HTTP {other}", redact_url(url)),
        }
    }

    pub(crate) async fn ensure_remote_directories(&self, collection: &str) -> Result<()> {
        for url in collection_urls(collection)? {
            self.ensure_one(&url).await?;
        }
        Ok(())
    }

    async fn ensure_one(&self, url: &str) -> Result<()> {
        if self.propfind_exists(url).await? {
            return Ok(());
        }
        log::info!("webdav.mkcol {}", redact_url(url));
        match self.mkcol(url).await? {
            MkcolOutcome::Created => Ok(()),
            MkcolOutcome::MaybeExists => {
                if self.propfind_exists(url).await? {
                    Ok(())
                } else {
                    anyhow::bail!(
                        "MKCOL {} returned 405/409 but collection is still missing",
                        redact_url(url)
                    )
                }
            }
        }
    }

    pub(crate) async fn get(&self, url: &str) -> Result<Option<Vec<u8>>> {
        let (status, body) = self.send(Method::GET, url, None, &[], true).await?;
        match status.as_u16() {
            200 => Ok(Some(body)),
            404 => Ok(None),
            401 | 403 => anyhow::bail!("webdav auth failed"),
            other => anyhow::bail!("GET {} failed: HTTP {other}", redact_url(url)),
        }
    }

    pub(crate) async fn put(&self, url: &str, body: &[u8]) -> Result<()> {
        let (status, _) = self
            .send(
                Method::PUT,
                url,
                Some(body.to_vec()),
                &[("Content-Type", "application/json")],
                true,
            )
            .await?;
        match status.as_u16() {
            200 | 201 | 204 => Ok(()),
            401 | 403 => anyhow::bail!("webdav auth failed"),
            other => anyhow::bail!("PUT {} failed: HTTP {other}", redact_url(url)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MkcolOutcome {
    Created,
    MaybeExists,
}

#[cfg(test)]
pub(crate) mod mock {
    use std::collections::{HashMap, HashSet};
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    #[derive(Default)]
    pub(crate) struct MockState {
        pub collections: HashSet<String>,
        pub files: HashMap<String, Vec<u8>>,
        pub log: Vec<String>,
        /// First PROPFIND for these paths returns 404 even if the collection exists.
        pub propfind_hide: HashSet<String>,
        pub mkcol_override: HashMap<String, u16>,
        pub put_fail: HashSet<String>,
    }

    pub(crate) struct MockServer {
        pub addr: SocketAddr,
        pub state: Arc<Mutex<MockState>>,
    }

    impl MockServer {
        pub(crate) fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock dav");
            let addr = listener.local_addr().expect("local addr");
            let state = Arc::new(Mutex::new(MockState::default()));
            let st = state.clone();
            thread::spawn(move || serve(listener, st));
            Self { addr, state }
        }

        pub(crate) fn collection_url(&self, path: &str) -> String {
            let path = if path.starts_with('/') {
                path.to_string()
            } else {
                format!("/{path}")
            };
            format!("http://{}{path}", self.addr)
        }

        pub(crate) fn methods(&self) -> Vec<String> {
            self.state.lock().expect("mock state").log.clone()
        }
    }

    fn norm(path: &str) -> String {
        let p = path.split('?').next().unwrap_or(path);
        if p.len() > 1 && p.ends_with('/') {
            p.trim_end_matches('/').to_string()
        } else {
            p.to_string()
        }
    }

    fn serve(listener: TcpListener, state: Arc<Mutex<MockState>>) {
        listener.set_nonblocking(false).ok();
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let st = state.clone();
            thread::spawn(move || {
                let _ = handle_conn(stream, st);
            });
        }
    }

    fn handle_conn(mut stream: TcpStream, state: Arc<Mutex<MockState>>) -> std::io::Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(3))).ok();
        while let Ok((method, path, req_body)) = read_request(&mut stream) {
            if method.is_empty() {
                break;
            }
            let path = norm(&path);
            let (code, body) = {
                let mut st = state.lock().expect("mock state");
                st.log.push(format!("{method} {path}"));
                respond(&mut st, &method, &path, &req_body)
            };
            write_response(&mut stream, code, &body)?;
        }
        Ok(())
    }

    fn respond(st: &mut MockState, method: &str, path: &str, body: &[u8]) -> (u16, Vec<u8>) {
        match method {
            "PROPFIND" => {
                if st.propfind_hide.remove(path) {
                    return (404, Vec::new());
                }
                if st.collections.contains(path) || st.files.contains_key(path) {
                    (207, b"<multistatus xmlns=\"DAV:\"/>".to_vec())
                } else {
                    (404, Vec::new())
                }
            }
            "MKCOL" => {
                if let Some(code) = st.mkcol_override.get(path).copied() {
                    if code == 201 || code == 200 {
                        st.collections.insert(path.to_string());
                    }
                    return (code, Vec::new());
                }
                if st.collections.contains(path) {
                    (405, Vec::new())
                } else {
                    st.collections.insert(path.to_string());
                    (201, Vec::new())
                }
            }
            "PUT" => {
                if st.put_fail.contains(path) {
                    return (500, Vec::new());
                }
                st.files.insert(path.to_string(), body.to_vec());
                (201, Vec::new())
            }
            "GET" => match st.files.get(path) {
                Some(b) => (200, b.clone()),
                None => (404, Vec::new()),
            },
            "HEAD" => {
                if st.files.contains_key(path) || st.collections.contains(path) {
                    (200, Vec::new())
                } else {
                    (404, Vec::new())
                }
            }
            _ => (405, Vec::new()),
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
        let mut expect_continue = false;
        for line in lines {
            let lower = line.to_ascii_lowercase();
            if let Some(v) = lower.strip_prefix("content-length:") {
                content_length = v.trim().parse().unwrap_or(0);
            }
            if lower.contains("expect:") && lower.contains("100-continue") {
                expect_continue = true;
            }
        }
        if expect_continue {
            stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
            stream.flush()?;
        }
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            stream.read_exact(&mut body)?;
        }
        Ok((method, path, body))
    }

    fn read_until_double_crlf(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        while buf.windows(4).all(|w| w != b"\r\n\r\n") {
            let n = stream.read(&mut byte)?;
            if n == 0 {
                break;
            }
            buf.push(byte[0]);
            if buf.len() > 64 * 1024 {
                break;
            }
        }
        Ok(buf)
    }

    fn write_response(stream: &mut TcpStream, code: u16, body: &[u8]) -> std::io::Result<()> {
        let reason = match code {
            200 => "OK",
            201 => "Created",
            204 => "No Content",
            207 => "Multi-Status",
            404 => "Not Found",
            405 => "Method Not Allowed",
            409 => "Conflict",
            _ => "OK",
        };
        let header = format!(
            "HTTP/1.1 {code} {reason}\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes())?;
        stream.write_all(body)?;
        stream.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::mock::MockServer;
    use super::*;

    #[test]
    fn rejects_non_localhost_http() {
        let err = validate_remote_url("http://example.com/dav").unwrap_err();
        assert!(err.to_string().contains("localhost"), "{err}");
        assert!(validate_remote_url("http://localhost:8080/dav").is_ok());
        assert!(validate_remote_url("http://127.0.0.1/dav").is_ok());
        assert!(validate_remote_url("https://webdav.example.com/").is_ok());
    }

    #[test]
    fn validate_does_not_append_namespace() {
        let url = "https://webdav.example.com/dav/";
        assert_eq!(validate_remote_url(url).unwrap(), url);
        assert!(!validate_remote_url(url).unwrap().contains("aimux-sync"));
    }

    #[test]
    fn join_file_appends_store() {
        let u = join_file("http://127.0.0.1:9/dav/sync", "store.json").unwrap();
        assert_eq!(u, "http://127.0.0.1:9/dav/sync/store.json");
        let u = join_file("http://127.0.0.1:9/dav/sync/", "manifest.json").unwrap();
        assert_eq!(u, "http://127.0.0.1:9/dav/sync/manifest.json");
    }

    #[test]
    fn join_dir_keeps_last_segment() {
        let u = join_dir("https://webdav.example.com/dav", "aimux-sync").unwrap();
        assert_eq!(u, "https://webdav.example.com/dav/aimux-sync");
        let u = join_dir("https://webdav.example.com/", "aimux-sync").unwrap();
        assert_eq!(u, "https://webdav.example.com/aimux-sync");
        let u = join_dir("https://webdav.example.com/", "").unwrap();
        assert_eq!(u, "https://webdav.example.com/");
        let err = join_dir("https://example.com/dav", "..").unwrap_err();
        assert!(err.to_string().contains(".."), "{err}");
    }

    #[test]
    fn namespaced_collection_appends_aimux_sync() {
        assert_eq!(
            namespaced_collection("https://webdav.example.com/dav/").unwrap(),
            "https://webdav.example.com/dav/aimux-sync"
        );
        assert_eq!(
            namespaced_collection("https://webdav.example.com").unwrap(),
            "https://webdav.example.com/aimux-sync"
        );
        assert_eq!(NAMESPACE, "aimux-sync");
    }

    #[test]
    fn mkcol_sent_to_mock_http_server() {
        let srv = MockServer::start();
        let url = srv.collection_url("/dav/aimux-sync");
        block_on(async {
            let client = DavClient::new("user", "secret")?;
            client.ensure_remote_directories(&url).await
        })
        .unwrap();
        let log = srv.methods();
        assert!(
            log.iter().any(|l| l.starts_with("MKCOL /dav/aimux-sync")),
            "expected MKCOL of user path, got {log:?}"
        );
        assert!(
            !log.iter()
                .any(|l| l.contains("/aimux") && !l.contains("/aimux-sync")),
            "must not invent /aimux: {log:?}"
        );
        let st = srv.state.lock().unwrap();
        assert!(st.collections.contains("/dav"));
        assert!(st.collections.contains("/dav/aimux-sync"));
    }

    #[test]
    fn mkcol_405_retries_propfind() {
        let srv = MockServer::start();
        {
            let mut st = srv.state.lock().unwrap();
            st.collections.insert("/dav".into());
            st.collections.insert("/dav/aimux-sync".into());
            st.propfind_hide.insert("/dav/aimux-sync".into());
            st.mkcol_override.insert("/dav/aimux-sync".into(), 405);
        }
        let url = srv.collection_url("/dav/aimux-sync");
        block_on(async {
            let client = DavClient::new("user", "secret")?;
            client.ensure_remote_directories(&url).await
        })
        .unwrap();
        let log = srv.methods();
        let mkcols: Vec<_> = log
            .iter()
            .filter(|l| l.starts_with("MKCOL"))
            .cloned()
            .collect();
        assert!(
            mkcols.iter().any(|l| l == "MKCOL /dav/aimux-sync"),
            "{log:?}"
        );
        let propfinds = log
            .iter()
            .filter(|l| l.as_str() == "PROPFIND /dav/aimux-sync")
            .count();
        assert!(propfinds >= 2, "405 should re-PROPFIND, got {log:?}");
    }
}
