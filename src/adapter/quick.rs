//! Built-in composable snippet fragments (cc-switch quick-config menus).
//!
//! The JSON on `Provider.snippet` is the SSOT. Users can edit it as a
//! file, or toggle these named fragments which merge in / unmerge out.

use serde_json::Value;

use super::merge;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuickItem {
    pub id: &'static str,
    pub label: &'static str,
    /// JSON object merged into `provider.snippet`. None = provider extra.
    pub snippet: Option<&'static str>,
    /// Provider extras key (`"true"` / removed). Codex remote compaction.
    pub extra_key: Option<&'static str>,
}

pub const CLAUDE: &[QuickItem] = &[
    QuickItem {
        id: "hide_attribution",
        label: "quick.hide_attribution",
        snippet: Some(r#"{"attribution":{"commit":"","pr":""}}"#),
        extra_key: None,
    },
    QuickItem {
        id: "teammates",
        label: "quick.teammates",
        snippet: Some(r#"{"env":{"CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS":"1"}}"#),
        extra_key: None,
    },
    QuickItem {
        id: "tool_search",
        label: "quick.tool_search",
        snippet: Some(r#"{"env":{"ENABLE_TOOL_SEARCH":"true"}}"#),
        extra_key: None,
    },
    QuickItem {
        id: "effort_max",
        label: "quick.effort_max",
        snippet: Some(r#"{"env":{"CLAUDE_CODE_EFFORT_LEVEL":"max"}}"#),
        extra_key: None,
    },
    QuickItem {
        id: "disable_autoupdate",
        label: "quick.disable_autoupdate",
        snippet: Some(r#"{"env":{"DISABLE_AUTOUPDATER":"1"}}"#),
        extra_key: None,
    },
    QuickItem {
        id: "unknown_model_reactive",
        label: "quick.unknown_model_reactive",
        snippet: Some(r#"{"env":{"CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT":"1"}}"#),
        extra_key: None,
    },
];

pub const CODEX: &[QuickItem] = &[
    QuickItem {
        id: "sandbox_network",
        label: "quick.sandbox_network",
        snippet: Some(r#"{"sandbox_workspace_write":{"network_access":true}}"#),
        extra_key: None,
    },
    QuickItem {
        id: "goal_mode",
        label: "quick.goal_mode",
        snippet: Some(r#"{"features":{"goals":true}}"#),
        extra_key: None,
    },
    QuickItem {
        id: "remote_compaction",
        label: "quick.remote_compaction",
        snippet: None,
        extra_key: Some("remote_compaction"),
    },
];

impl QuickItem {
    pub fn fragment(&self) -> Option<Value> {
        self.snippet
            .map(|raw| serde_json::from_str(raw).expect("static quick fragment"))
    }

    pub fn snippet_on(&self, snippet: &Value) -> bool {
        match self.fragment() {
            Some(frag) => fragment_is_on(snippet, &frag),
            None => false,
        }
    }

    pub fn extra_on(&self, extras: &std::collections::BTreeMap<String, String>) -> bool {
        let Some(key) = self.extra_key else {
            return false;
        };
        matches!(
            extras.get(key).map(String::as_str),
            Some("true") | Some("yes") | Some("1")
        )
    }

    pub fn apply_snippet(&self, snippet: &mut Value) {
        if let Some(frag) = self.fragment() {
            if !snippet.is_object() {
                *snippet = Value::Object(serde_json::Map::new());
            }
            merge::json_merge(snippet, &frag);
        }
    }

    pub fn remove_snippet(&self, snippet: &mut Value) {
        if let Some(frag) = self.fragment() {
            merge::json_unmerge(snippet, &frag);
        }
    }
}

fn fragment_is_on(hay: &Value, needle: &Value) -> bool {
    match (hay, needle) {
        (Value::Object(h), Value::Object(n)) => n
            .iter()
            .all(|(k, nv)| h.get(k).is_some_and(|hv| fragment_is_on(hv, nv))),
        (Value::String(a), Value::String(b)) => loose_eq(a, b),
        (Value::String(a), Value::Number(n)) if n.as_i64() == Some(1) => {
            a == "1" || a.eq_ignore_ascii_case("true")
        }
        (Value::Number(n), Value::String(b)) if n.as_i64() == Some(1) => {
            b == "1" || b.eq_ignore_ascii_case("true")
        }
        (a, b) => a == b,
    }
}

fn loose_eq(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let al = a.eq_ignore_ascii_case("true") || a == "1";
    let bl = b.eq_ignore_ascii_case("true") || b == "1";
    al && bl
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_toggles_merge_and_unmerge_env_keys() {
        let mut snippet = json!({});
        let item = CLAUDE.iter().find(|i| i.id == "teammates").unwrap();
        assert!(!item.snippet_on(&snippet));
        item.apply_snippet(&mut snippet);
        assert!(item.snippet_on(&snippet));
        assert_eq!(snippet["env"]["CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"], "1");
        item.remove_snippet(&mut snippet);
        assert!(!item.snippet_on(&snippet));
        assert!(snippet.get("env").is_none());
    }

    #[test]
    fn unmerge_leaves_other_env_keys() {
        let mut snippet = json!({
            "env": {
                "FOO": "bar",
                "ENABLE_TOOL_SEARCH": "true"
            }
        });
        let item = CLAUDE.iter().find(|i| i.id == "tool_search").unwrap();
        assert!(item.snippet_on(&snippet));
        item.remove_snippet(&mut snippet);
        assert_eq!(snippet["env"]["FOO"], "bar");
        assert!(snippet["env"].get("ENABLE_TOOL_SEARCH").is_none());
    }

    #[test]
    fn unknown_model_reactive_sets_enforcement_off() {
        let mut snippet = json!({});
        let item = CLAUDE
            .iter()
            .find(|i| i.id == "unknown_model_reactive")
            .unwrap();
        assert!(!item.snippet_on(&snippet));
        item.apply_snippet(&mut snippet);
        assert_eq!(
            snippet["env"]["CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT"],
            "1"
        );
        item.remove_snippet(&mut snippet);
        assert!(snippet.get("env").is_none());
    }

    #[test]
    fn hide_attribution_matches_empty_commit_and_pr() {
        let item = CLAUDE.iter().find(|i| i.id == "hide_attribution").unwrap();
        let on = json!({"attribution":{"commit":"","pr":""}});
        assert!(item.snippet_on(&on));
        assert!(!item.snippet_on(&json!({})));
    }

    #[test]
    fn codex_goal_mode_fragment() {
        let item = CODEX.iter().find(|i| i.id == "goal_mode").unwrap();
        let mut snippet = json!({});
        item.apply_snippet(&mut snippet);
        assert_eq!(snippet["features"]["goals"], true);
    }

    #[test]
    fn codex_sandbox_network_fragment() {
        let item = CODEX.iter().find(|i| i.id == "sandbox_network").unwrap();
        let mut snippet = json!({});
        assert!(!item.snippet_on(&snippet));
        item.apply_snippet(&mut snippet);
        assert!(item.snippet_on(&snippet));
        assert_eq!(snippet["sandbox_workspace_write"]["network_access"], true);
        item.remove_snippet(&mut snippet);
        assert!(!item.snippet_on(&snippet));
        assert!(snippet.get("sandbox_workspace_write").is_none());
    }
}
