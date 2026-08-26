use anyhow::{anyhow, Result};
use serde_json::Value;
use toml_edit::{value as toml_value, DocumentMut, Item, TableLike};

/// Deep-merge `overlay` into `base`. Objects merge recursively; other values replace.
pub fn json_merge(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(over_map)) => {
            for (k, v) in over_map {
                match base_map.get_mut(k) {
                    Some(existing) => json_merge(existing, v),
                    None => {
                        base_map.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

/// Remove keys that match `fragment`. Nested objects are trimmed; empty parents drop.
pub fn json_unmerge(base: &mut Value, fragment: &Value) {
    let Value::Object(frag) = fragment else {
        return;
    };
    let Some(base_obj) = base.as_object_mut() else {
        return;
    };
    let keys: Vec<String> = frag.keys().cloned().collect();
    for k in keys {
        let fv = &frag[&k];
        match base_obj.get_mut(&k) {
            Some(sv) if fv.is_object() && sv.is_object() => {
                json_unmerge(sv, fv);
                if sv.as_object().is_some_and(|o| o.is_empty()) {
                    base_obj.remove(&k);
                }
            }
            Some(sv) if values_match(sv, fv) => {
                base_obj.remove(&k);
            }
            _ => {}
        }
    }
}

fn values_match(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    match (a, b) {
        (Value::String(s), Value::Number(n)) if n.as_i64() == Some(1) => {
            s == "1" || s.eq_ignore_ascii_case("true")
        }
        (Value::Number(n), Value::String(s)) if n.as_i64() == Some(1) => {
            s == "1" || s.eq_ignore_ascii_case("true")
        }
        (Value::String(a), Value::String(b)) => {
            let al = a.eq_ignore_ascii_case("true") || a == "1";
            let bl = b.eq_ignore_ascii_case("true") || b == "1";
            al && bl
        }
        _ => false,
    }
}

/// Set `value` at `path`, creating missing intermediate objects along that path only.
pub fn json_set(root: &mut Value, path: &[&str], value: Value) -> Result<()> {
    if !root.is_object() {
        anyhow::bail!("expected JSON object");
    }
    let Some((last, parents)) = path.split_last() else {
        anyhow::bail!("empty JSON path");
    };
    let mut cur = root;
    for (i, key) in parents.iter().enumerate() {
        let obj = cur
            .as_object_mut()
            .ok_or_else(|| anyhow!("expected JSON object"))?;
        if !obj.contains_key(*key) {
            obj.insert((*key).to_string(), Value::Object(serde_json::Map::new()));
        } else if !obj[*key].is_object() {
            anyhow::bail!("expected object at {}", path[..=i].join("."));
        }
        cur = obj.get_mut(*key).expect("key just ensured");
    }
    let obj = cur
        .as_object_mut()
        .ok_or_else(|| anyhow!("expected JSON object"))?;
    obj.insert((*last).to_string(), value);
    Ok(())
}

/// Remove the last key of `path`. Missing keys are a no-op; a non-object parent is an error.
pub fn json_remove(root: &mut Value, path: &[&str]) -> Result<()> {
    if !root.is_object() {
        anyhow::bail!("expected JSON object");
    }
    let Some((last, parents)) = path.split_last() else {
        anyhow::bail!("empty JSON path");
    };
    let mut cur = root;
    for (i, key) in parents.iter().enumerate() {
        let obj = cur
            .as_object_mut()
            .ok_or_else(|| anyhow!("expected JSON object"))?;
        match obj.get(*key) {
            None => return Ok(()),
            Some(v) if v.is_object() => {}
            Some(_) => anyhow::bail!("expected object at {}", path[..=i].join(".")),
        }
        cur = obj.get_mut(*key).expect("key exists");
    }
    if let Some(obj) = cur.as_object_mut() {
        obj.remove(*last);
    }
    Ok(())
}

/// Merge a JSON object into a TOML document (objects → tables, other values replace).
pub fn toml_merge_json(doc: &mut DocumentMut, overlay: &Value) {
    if let Some(obj) = overlay.as_object() {
        merge_toml_table(doc.as_table_mut(), obj);
    }
}

fn merge_toml_table(table: &mut dyn TableLike, overlay: &serde_json::Map<String, Value>) {
    for (k, v) in overlay {
        match v {
            Value::Object(child) => {
                if table.get(k).and_then(|i| i.as_table_like()).is_none() {
                    table.insert(k, toml_edit::table());
                }
                if let Some(item) = table.get_mut(k) {
                    if let Some(t) = item.as_table_like_mut() {
                        merge_toml_table(t, child);
                    }
                }
            }
            Value::String(s) => {
                table.insert(k, toml_value(s.as_str()));
            }
            Value::Bool(b) => {
                table.insert(k, toml_value(*b));
            }
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    table.insert(k, toml_value(i));
                } else if let Some(f) = n.as_f64() {
                    table.insert(k, toml_value(f));
                }
            }
            _ => {}
        }
    }
}

/// Set `item` at `path`, creating missing intermediate tables along that path only.
pub fn toml_set(doc: &mut DocumentMut, path: &[&str], item: Item) -> Result<()> {
    if path.is_empty() {
        anyhow::bail!("empty TOML path");
    }
    insert_path(doc.as_table_mut(), path, item, path, 0)
}

/// Remove the last key of `path`. Missing keys are a no-op; a non-table parent is an error.
pub fn toml_remove(doc: &mut DocumentMut, path: &[&str]) -> Result<()> {
    if path.is_empty() {
        anyhow::bail!("empty TOML path");
    }
    remove_path(doc.as_table_mut(), path, path, 0)
}

fn insert_path<T: TableLike + ?Sized>(
    table: &mut T,
    path: &[&str],
    item: Item,
    full: &[&str],
    offset: usize,
) -> Result<()> {
    let Some((first, rest)) = path.split_first() else {
        anyhow::bail!("empty TOML path");
    };
    if rest.is_empty() {
        table.insert(first, item);
        return Ok(());
    }
    if table.get(first).is_none() {
        table.insert(first, toml_edit::table());
    }
    let child = table.get_mut(first).expect("key just ensured");
    let child_table = child
        .as_table_like_mut()
        .ok_or_else(|| anyhow!("expected table at {}", full[..=offset].join(".")))?;
    insert_path(child_table, rest, item, full, offset + 1)
}

fn remove_path<T: TableLike + ?Sized>(
    table: &mut T,
    path: &[&str],
    full: &[&str],
    offset: usize,
) -> Result<()> {
    let Some((first, rest)) = path.split_first() else {
        anyhow::bail!("empty TOML path");
    };
    if rest.is_empty() {
        table.remove(first);
        return Ok(());
    }
    match table.get(first) {
        None => return Ok(()),
        Some(item) if item.as_table_like().is_some() => {}
        Some(_) => anyhow::bail!("expected table at {}", full[..=offset].join(".")),
    }
    let child = table.get_mut(first).expect("key exists");
    let child_table = child
        .as_table_like_mut()
        .ok_or_else(|| anyhow!("expected table at {}", full[..=offset].join(".")))?;
    remove_path(child_table, rest, full, offset + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_unmerge_trims_matching_leaves() {
        let mut base = json!({"env": {"FOO": "bar", "X": "1"}, "n": 1});
        json_unmerge(&mut base, &json!({"env": {"X": "1"}}));
        assert_eq!(base, json!({"env": {"FOO": "bar"}, "n": 1}));
        json_unmerge(&mut base, &json!({"n": 1}));
        assert_eq!(base, json!({"env": {"FOO": "bar"}}));
    }

    #[test]
    fn json_merge_objects_then_replaces_scalars() {
        let mut base = json!({"env": {"FOO": "old", "KEEP": "yes"}, "n": 1});
        json_merge(
            &mut base,
            &json!({"env": {"FOO": "new", "BAR": "x"}, "n": 2, "extra": true}),
        );
        assert_eq!(
            base,
            json!({"env": {"FOO": "new", "KEEP": "yes", "BAR": "x"}, "n": 2, "extra": true})
        );
    }

    #[test]
    fn set_creates_intermediate_objects() {
        let mut doc = json!({});
        json_set(&mut doc, &["env", "FOO"], json!("bar")).unwrap();
        assert_eq!(doc, json!({"env": {"FOO": "bar"}}));
    }

    #[test]
    fn set_preserves_unrelated_keys() {
        let mut doc = json!({"permissions": {"allow": ["Bash"]}, "env": {"FOO": "bar"}});
        json_set(&mut doc, &["env", "ANTHROPIC_BASE_URL"], json!("https://x")).unwrap();
        assert_eq!(doc["permissions"]["allow"][0], "Bash");
        assert_eq!(doc["env"]["FOO"], "bar");
        assert_eq!(doc["env"]["ANTHROPIC_BASE_URL"], "https://x");
    }

    #[test]
    fn remove_missing_is_ok() {
        let mut doc = json!({"env": {"FOO": "bar"}});
        json_remove(&mut doc, &["env", "ANTHROPIC_MODEL"]).unwrap();
        assert_eq!(doc, json!({"env": {"FOO": "bar"}}));
        json_remove(&mut doc, &["missing", "key"]).unwrap();
        assert_eq!(doc["env"]["FOO"], "bar");
    }

    #[test]
    fn non_object_parent_is_error() {
        let mut doc = json!({"env": "not-an-object"});
        let err = json_set(&mut doc, &["env", "FOO"], json!("x")).unwrap_err();
        assert!(err.to_string().contains("expected object at env"));
        let err = json_remove(&mut doc, &["env", "FOO"]).unwrap_err();
        assert!(err.to_string().contains("expected object at env"));
    }

    #[test]
    fn root_must_be_object() {
        let mut doc = json!([]);
        assert!(json_set(&mut doc, &["a"], json!(1)).is_err());
        assert!(json_remove(&mut doc, &["a"]).is_err());
    }

    #[test]
    fn toml_set_creates_intermediate_tables() {
        let mut doc = DocumentMut::new();
        toml_set(
            &mut doc,
            &["model_providers", "managed", "name"],
            toml_edit::value("Packy"),
        )
        .unwrap();
        assert_eq!(
            doc["model_providers"]["managed"]["name"].as_str(),
            Some("Packy")
        );
    }

    #[test]
    fn toml_set_preserves_unrelated_tables() {
        let mut doc: DocumentMut = r#"
[mcp_servers.docs]
command = "uvx"

[model_providers.openai]
name = "OpenAI"
"#
        .parse()
        .unwrap();
        toml_set(
            &mut doc,
            &["model_providers", "managed", "base_url"],
            toml_edit::value("https://x"),
        )
        .unwrap();
        assert_eq!(doc["mcp_servers"]["docs"]["command"].as_str(), Some("uvx"));
        assert_eq!(
            doc["model_providers"]["openai"]["name"].as_str(),
            Some("OpenAI")
        );
        assert_eq!(
            doc["model_providers"]["managed"]["base_url"].as_str(),
            Some("https://x")
        );
    }

    #[test]
    fn toml_remove_missing_is_ok() {
        let mut doc = DocumentMut::new();
        toml_set(&mut doc, &["model"], toml_edit::value("gpt")).unwrap();
        toml_remove(&mut doc, &["model"]).unwrap();
        assert!(doc.get("model").is_none());
        toml_remove(&mut doc, &["model"]).unwrap();
        toml_remove(&mut doc, &["model_providers", "managed", "name"]).unwrap();
    }

    #[test]
    fn toml_non_table_parent_is_error() {
        let mut doc = DocumentMut::new();
        toml_set(&mut doc, &["model_providers"], toml_edit::value("nope")).unwrap();
        let err = toml_set(
            &mut doc,
            &["model_providers", "managed", "name"],
            toml_edit::value("x"),
        )
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("expected table at model_providers"));
        let err = toml_remove(&mut doc, &["model_providers", "managed"]).unwrap_err();
        assert!(err
            .to_string()
            .contains("expected table at model_providers"));
    }

    #[test]
    fn toml_empty_path_is_error() {
        let mut doc = DocumentMut::new();
        assert!(toml_set(&mut doc, &[], toml_edit::value("x")).is_err());
        assert!(toml_remove(&mut doc, &[]).is_err());
    }
}
