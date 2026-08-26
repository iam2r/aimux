//! Model UI shape, catalog/slot helpers, and remote model-list fetch.

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde_json::Value;

use crate::store::{ModelEntry, Provider};
use crate::webdav;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogField {
    Id,
    Label,
    ContextWindow,
    MaxTokens,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub env_key: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelUi {
    Catalog { fields: &'static [CatalogField] },
    Slots { slots: &'static [SlotSpec] },
}

pub const OPENCODE_FIELDS: &[CatalogField] = &[
    CatalogField::Id,
    CatalogField::Label,
    CatalogField::ContextWindow,
    CatalogField::MaxTokens,
];

pub const PI_FIELDS: &[CatalogField] = OPENCODE_FIELDS;

pub const CODEX_FIELDS: &[CatalogField] = &[
    CatalogField::Id,
    CatalogField::Label,
    CatalogField::ContextWindow,
];

pub const CLAUDE_SLOTS: &[SlotSpec] = &[
    SlotSpec {
        key: "haiku",
        label: "slot.haiku",
        env_key: "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    },
    SlotSpec {
        key: "sonnet",
        label: "slot.sonnet",
        env_key: "ANTHROPIC_DEFAULT_SONNET_MODEL",
    },
    SlotSpec {
        key: "opus",
        label: "slot.opus",
        env_key: "ANTHROPIC_DEFAULT_OPUS_MODEL",
    },
    SlotSpec {
        key: "fable",
        label: "slot.fable",
        env_key: "ANTHROPIC_DEFAULT_FABLE_MODEL",
    },
    SlotSpec {
        key: "subagent",
        label: "slot.subagent",
        env_key: "CLAUDE_CODE_SUBAGENT_MODEL",
    },
];

pub fn catalog_models(provider: &Provider) -> Vec<ModelEntry> {
    if !provider.catalog.is_empty() {
        return provider.catalog.clone();
    }
    match provider
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(id) => vec![ModelEntry {
            id: id.to_string(),
            ..ModelEntry::default()
        }],
        None => Vec::new(),
    }
}

pub fn known_slot(key: &str) -> Option<&'static SlotSpec> {
    CLAUDE_SLOTS.iter().find(|s| s.key == key)
}

pub fn fetch_models(
    base_url: &str,
    api_key: &str,
    extra_protocol: Option<&str>,
) -> Result<Vec<String>> {
    webdav::block_on(fetch_models_async(base_url, api_key, extra_protocol))
}

async fn fetch_models_async(
    base_url: &str,
    api_key: &str,
    protocol: Option<&str>,
) -> Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("http client")?;
    let mut last_err = None;
    for url in candidate_urls(base_url)? {
        match fetch_one(&client, &url, api_key, protocol).await {
            Ok(ids) if !ids.is_empty() => return Ok(ids),
            Ok(_) => last_err = Some(anyhow::anyhow!("empty model list from {url}")),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no model list endpoint responded")))
}

fn candidate_urls(base_url: &str) -> Result<Vec<String>> {
    let raw = base_url.trim().trim_end_matches('/');
    let url = reqwest::Url::parse(raw).context("invalid base_url")?;
    let mut out = Vec::new();
    if url.path().ends_with("/models") {
        out.push(raw.to_string());
    } else {
        out.push(format!("{raw}/models"));
        if !url.path().ends_with("/v1") {
            out.push(format!("{raw}/v1/models"));
        }
    }
    Ok(out)
}

async fn fetch_one(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    protocol: Option<&str>,
) -> Result<Vec<String>> {
    let mut headers = HeaderMap::new();
    if !api_key.is_empty() {
        if protocol == Some("anthropic") {
            headers.insert(
                "x-api-key",
                HeaderValue::from_str(api_key).context("api key header")?,
            );
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        } else {
            let value = format!("Bearer {api_key}");
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&value).context("authorization header")?,
            );
        }
    }
    let body = client
        .get(url)
        .headers(headers)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("GET {url}"))?
        .json::<Value>()
        .await
        .with_context(|| format!("parse {url}"))?;
    let ids = parse_model_ids(&body);
    if ids.is_empty() {
        anyhow::bail!("no model ids in {url}");
    }
    Ok(ids)
}

pub fn parse_model_ids(body: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    let arrays = [
        body.get("data").and_then(Value::as_array),
        body.get("models").and_then(Value::as_array),
        body.as_array(),
    ];
    for arr in arrays.into_iter().flatten() {
        for item in arr {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                push_id(&mut ids, id);
            } else if let Some(id) = item.get("name").and_then(Value::as_str) {
                push_id(&mut ids, id);
            } else if let Some(id) = item.as_str() {
                push_id(&mut ids, id);
            }
        }
        if !ids.is_empty() {
            break;
        }
    }
    ids
}

fn push_id(ids: &mut Vec<String>, id: &str) {
    let id = id.trim();
    if id.is_empty() {
        return;
    }
    if !ids.iter().any(|e| e == id) {
        ids.push(id.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_openai_data_array() {
        let body = json!({"data": [{"id": "gpt-4o"}, {"id": "o3"}]});
        assert_eq!(parse_model_ids(&body), vec!["gpt-4o", "o3"]);
    }

    #[test]
    fn parse_models_array_and_strings() {
        let body = json!({"models": ["a", {"name": "b"}]});
        assert_eq!(parse_model_ids(&body), vec!["a", "b"]);
    }

    #[test]
    fn catalog_models_falls_back_to_default() {
        let mut p = Provider::blank(crate::store::AppId::OpenCode);
        p.model = Some("gpt-4o".into());
        let rows = catalog_models(&p);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "gpt-4o");
        p.catalog = vec![ModelEntry {
            id: "o3".into(),
            label: Some("o3".into()),
            ..ModelEntry::default()
        }];
        assert_eq!(catalog_models(&p)[0].id, "o3");
    }

    #[test]
    fn known_slot_keys() {
        assert!(known_slot("haiku").is_some());
        assert!(known_slot("subagent").is_some());
        assert!(known_slot("nope").is_none());
    }

    #[test]
    fn fetch_models_from_mock_http() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let body = r#"{"data":[{"id":"gpt-4o"},{"id":"o3"}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        let ids = fetch_models(&format!("http://{addr}"), "sk-test", None).unwrap();
        assert_eq!(ids, vec!["gpt-4o", "o3"]);
    }

    #[test]
    fn candidate_urls_try_models_then_v1() {
        let urls = candidate_urls("https://api.example.com").unwrap();
        assert_eq!(
            urls,
            vec![
                "https://api.example.com/models".to_string(),
                "https://api.example.com/v1/models".to_string()
            ]
        );
        let urls = candidate_urls("https://api.example.com/v1").unwrap();
        assert_eq!(urls, vec!["https://api.example.com/v1/models".to_string()]);
    }
}
