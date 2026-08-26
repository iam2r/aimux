//! Shared TUI/CLI protocol for OpenCode and Pi.
//!
//! Store extras key is always `protocol`. Live files stay client-specific:
//! OpenCode `npm`, Pi `api`. OpenCode has no Google option; Pi does.

use std::collections::BTreeMap;

use anyhow::Result;

use super::{FieldKind, FieldSpec, FieldStorage};

pub const OPENCODE: &[&str] = &["openai-completions", "openai-responses", "anthropic"];
pub const PI: &[&str] = &[
    "openai-completions",
    "openai-responses",
    "anthropic",
    "google",
];

pub const DEFAULT: &str = "openai-completions";

pub const OPENCODE_FIELD: FieldSpec = FieldSpec {
    key: "protocol",
    label: "field.protocol",
    kind: FieldKind::Select(OPENCODE),
    required: false,
    default: Some(DEFAULT),
    storage: FieldStorage::Extra("protocol"),
};

pub const PI_FIELD: FieldSpec = FieldSpec {
    key: "protocol",
    label: "field.protocol",
    kind: FieldKind::Select(PI),
    required: false,
    default: Some(DEFAULT),
    storage: FieldStorage::Extra("protocol"),
};

const OPENAI_NPM: &str = "@ai-sdk/openai";
const OPENAI_COMPAT_NPM: &str = "@ai-sdk/openai-compatible";
const ANTHROPIC_NPM: &str = "@ai-sdk/anthropic";
const GOOGLE_NPM: &str = "@ai-sdk/google";

pub fn from_extras(extras: &BTreeMap<String, String>) -> Result<&'static str> {
    if let Some(p) = extras.get("protocol") {
        return named(p).ok_or_else(|| anyhow::anyhow!("invalid protocol: {p}"));
    }
    if let Some(api) = extras.get("api") {
        return match api.as_str() {
            "openai-completions" => Ok("openai-completions"),
            "openai-responses" => Ok("openai-responses"),
            "anthropic-messages" => Ok("anthropic"),
            "google-generative-ai" => Ok("google"),
            other => anyhow::bail!("invalid api: {other}"),
        };
    }
    if let Some(npm) = extras.get("npm") {
        return Ok(match npm.as_str() {
            OPENAI_NPM => "openai-responses",
            ANTHROPIC_NPM => "anthropic",
            GOOGLE_NPM => "google",
            _ => DEFAULT,
        });
    }
    Ok(DEFAULT)
}

pub fn require_allowed(protocol: &str, allowed: &[&str]) -> Result<()> {
    if allowed.contains(&protocol) {
        Ok(())
    } else {
        anyhow::bail!("invalid protocol: {protocol}")
    }
}

fn named(value: &str) -> Option<&'static str> {
    PI.iter().copied().find(|v| *v == value)
}

pub fn pi_api(protocol: &str) -> &'static str {
    match protocol {
        "openai-responses" => "openai-responses",
        "anthropic" => "anthropic-messages",
        "google" => "google-generative-ai",
        _ => "openai-completions",
    }
}

pub fn opencode_npm(protocol: &str) -> &'static str {
    match protocol {
        "openai-responses" => OPENAI_NPM,
        "anthropic" => ANTHROPIC_NPM,
        _ => OPENAI_COMPAT_NPM,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extras(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn protocol_wins_over_legacy_keys() {
        let e = extras(&[
            ("protocol", "google"),
            ("api", "openai-responses"),
            ("npm", ANTHROPIC_NPM),
        ]);
        assert_eq!(from_extras(&e).unwrap(), "google");
        require_allowed("google", OPENCODE).unwrap_err();
        require_allowed("google", PI).unwrap();
    }

    #[test]
    fn legacy_pi_api_and_opencode_npm() {
        assert_eq!(
            from_extras(&extras(&[("api", "anthropic-messages")])).unwrap(),
            "anthropic"
        );
        assert_eq!(
            from_extras(&extras(&[("npm", GOOGLE_NPM)])).unwrap(),
            "google"
        );
        assert_eq!(
            from_extras(&extras(&[("npm", OPENAI_NPM)])).unwrap(),
            "openai-responses"
        );
        assert_eq!(
            from_extras(&extras(&[("npm", OPENAI_COMPAT_NPM)])).unwrap(),
            "openai-completions"
        );
        assert_eq!(from_extras(&BTreeMap::new()).unwrap(), DEFAULT);
    }

    #[test]
    fn live_mapping() {
        assert_eq!(pi_api("openai-completions"), "openai-completions");
        assert_eq!(pi_api("openai-responses"), "openai-responses");
        assert_eq!(pi_api("anthropic"), "anthropic-messages");
        assert_eq!(pi_api("google"), "google-generative-ai");
        assert_eq!(opencode_npm("openai-completions"), OPENAI_COMPAT_NPM);
        assert_eq!(opencode_npm("openai-responses"), OPENAI_NPM);
        assert_eq!(opencode_npm("anthropic"), ANTHROPIC_NPM);
    }
}
