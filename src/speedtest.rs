//! Provider connectivity test (`aimux test` / TUI `t`): time a real request
//! against the provider's base_url so "is this relay any good" stops being
//! guesswork. Follows cc-switch-cli's approach — a warm-up request absorbs
//! TLS/DNS/connection setup, then a second timed request measures steady
//! state latency.

use crate::store::{AppId, Store};
use anyhow::{Context, Result};
use std::time::{Duration, Instant};

const TIMEOUT_SECS: u64 = 10;
/// Warm-up + timed request per endpoint.
#[derive(Debug)]
pub struct SpeedResult {
    pub app: AppId,
    pub name: String,
    pub url: String,
    pub latency: Option<Duration>,
    pub status: Option<u16>,
    pub error: Option<String>,
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .user_agent(concat!(env!("CARGO_PKG_NAME"), "-speedtest/1.0"))
        .build()?)
}

async fn probe(
    client: &reqwest::Client,
    url: &str,
) -> (Option<Duration>, Option<u16>, Option<String>) {
    let parsed = match reqwest::Url::parse(url) {
        Ok(u) => u,
        Err(e) => return (None, None, Some(format!("invalid url: {e}"))),
    };
    // Warm-up: connection pool absorbs DNS/TLS/handshake cost; errors ignored.
    let _ = client.get(parsed.clone()).send().await;

    let start = Instant::now();
    match client.get(parsed).send().await {
        Ok(resp) => {
            let latency = start.elapsed();
            let status = resp.status().as_u16();
            // Drain the body so keep-alive state stays realistic for the
            // next probe without allocating for payloads we don't need.
            let _ = resp.bytes().await;
            (Some(latency), Some(status), None)
        }
        Err(e) => (None, None, Some(format!("{e}"))),
    }
}

/// Probe one provider by display name. Uses the same resolution as
/// `aimux use`, so names/ids/substrings all work.
pub async fn test_provider(store: &Store, query: &str) -> Result<SpeedResult> {
    let provider = crate::switch::resolve(store, query, None)?;
    test_provider_inner(&client()?, provider).await
}

async fn test_provider_inner(
    client: &reqwest::Client,
    provider: &crate::store::Provider,
) -> Result<SpeedResult> {
    if provider.official {
        anyhow::bail!(
            "'{}' is the official native-login row — there is no custom endpoint to probe",
            provider.name
        );
    }
    let url = provider.base_url.trim().to_string();
    if url.is_empty() {
        anyhow::bail!("provider '{}' has no base_url to test", provider.name);
    }
    let (latency, status, error) = probe(client, &url).await;
    Ok(SpeedResult {
        app: provider.app,
        name: provider.name.clone(),
        url,
        latency,
        status,
        error,
    })
}

/// Probe by exact provider id (TUI path — no name ambiguity).
pub async fn test_provider_by_id(store: &Store, id: &str) -> Result<SpeedResult> {
    let provider = store
        .providers
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("provider '{id}' not found"))?;
    test_provider_inner(&client()?, provider).await
}

/// CLI rendering: one line per probe with a human latency/status verdict.
pub fn render_result(r: &SpeedResult) -> String {
    let mut out = format!("[{}/{}] {}", r.app, r.name, r.url);
    match (&r.latency, &r.error) {
        (Some(l), _) => {
            out.push_str(&format!("  {} ms", l.as_millis()));
            if let Some(s) = r.status {
                out.push_str(&format!("  (HTTP {s})"));
            }
        }
        (None, Some(e)) => out.push_str(&format!("  FAILED: {e}")),
        (None, None) => out.push_str("  timeout"),
    }
    out.push('\n');
    out
}

/// Resolve + validate up front (sync), then probe. Used by main to surface
/// resolution errors before entering the runtime.
pub fn run(store: &Store, query: &str) -> Result<SpeedResult> {
    // Validate resolvability synchronously for a clean error path.
    crate::switch::resolve(store, query, None)?;
    crate::webdav::block_on(async { test_provider(store, query).await }).context("speedtest failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{AppId, Provider};

    fn provider_at(name: &str, url: &str) -> Provider {
        Provider {
            id: format!("codex-{name}"),
            name: name.into(),
            app: AppId::Codex,
            base_url: url.into(),
            api_key: "sk-test".into(),
            ..Provider::blank(AppId::Codex)
        }
    }

    #[test]
    fn probes_live_endpoint() {
        use crate::webdav::mock::MockServer;
        let srv = MockServer::start();
        let mut store = crate::store::Store::empty();
        let url = srv.collection_url("/v1");
        store
            .providers
            .insert("codex-pk".into(), provider_at("pk", &url));
        let result = crate::webdav::block_on(async { test_provider(&store, "pk").await }).unwrap();
        // Any HTTP status proves reachability; 404 on a bare path is normal.
        assert!(result.status.is_some(), "{result:?}");
        assert!(result.error.is_none());
        assert!(result.latency.is_some());
        assert!(result.latency.unwrap() < Duration::from_secs(TIMEOUT_SECS));
    }

    #[test]
    fn official_row_has_nothing_to_probe() {
        let mut store = crate::store::Store::empty();
        let mut p = provider_at("official", "");
        p.official = true;
        store.providers.insert(p.id.clone(), p);
        let err = format!("{:#}", run(&store, "official").unwrap_err());
        assert!(err.contains("native-login"), "{err}");
    }

    #[test]
    fn unreachable_url_reports_error_not_panic() {
        let mut store = crate::store::Store::empty();
        store.providers.insert(
            "codex-dead".into(),
            provider_at("dead", "http://127.0.0.1:1/v1"),
        );
        // Bypass system proxies (HTTP_PROXY etc.): through a local proxy a
        // dead backend answers 502 and looks reachable. We need a real
        // connection failure here.
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .build()
            .unwrap();
        let provider = store.providers.get("codex-dead").unwrap();
        let result =
            crate::webdav::block_on(async { test_provider_inner(&client, provider).await })
                .unwrap();
        assert!(result.latency.is_none());
        assert!(result.error.is_some(), "{result:?}");
    }

    #[test]
    fn render_formats_latency_and_failure() {
        let ok = SpeedResult {
            app: AppId::Codex,
            name: "pk".into(),
            url: "https://x.example.com".into(),
            latency: Some(Duration::from_millis(123)),
            status: Some(200),
            error: None,
        };
        let line = render_result(&ok);
        assert!(line.contains("123 ms"), "{line}");
        assert!(line.contains("HTTP 200"), "{line}");

        let bad = SpeedResult {
            latency: None,
            status: None,
            error: Some("connection refused".into()),
            ..ok
        };
        let line = render_result(&bad);
        assert!(
            line.contains("FAILED") && line.contains("connection refused"),
            "{line}"
        );
    }
}
