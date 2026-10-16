//! Response shaping: API-style JSON is mostly noise to a model — link templates,
//! nulls, empty collections, pretty-print whitespace. Strip it before it reaches context.

use serde_json::Value;

/// Keys dropped everywhere. `*_url` covers GitHub-style hypermedia links;
/// `html_url` and `url` survive because they're what a person would click.
fn is_noise_key(key: &str, extra: &[String]) -> bool {
    matches!(key, "node_id" | "gravatar_id" | "_links" | "etag" | "avatar_url")
        || (key.ends_with("_url") && key != "html_url")
        || extra.iter().any(|k| k == key)
}

fn is_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

fn shape(value: Value, max_items: usize, extra: &[String]) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(k, _)| !is_noise_key(k, extra))
                .map(|(k, v)| (k, shape(v, max_items, extra)))
                .filter(|(_, v)| !is_empty(v))
                .collect(),
        ),
        Value::Array(items) => {
            let total = items.len();
            let mut kept: Vec<Value> =
                items.into_iter().take(max_items).map(|v| shape(v, max_items, extra)).collect();
            if total > max_items {
                kept.push(Value::String(format!(
                    "[mcp-router: {} more items omitted; narrow the query or paginate]",
                    total - max_items
                )));
            }
            Value::Array(kept)
        }
        other => other,
    }
}

/// Shape `text` if it is JSON; anything else is returned unchanged.
pub fn shape_text(text: &str, max_items: usize, extra: &[String]) -> Option<String> {
    let trimmed = text.trim_start();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    let value: Value = serde_json::from_str(text).ok()?;
    serde_json::to_string(&shape(value, max_items, extra)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strips_noise_and_minifies() {
        let issue = json!({
            "url": "https://api.github.com/repos/o/r/issues/1",
            "repository_url": "https://api.github.com/repos/o/r",
            "html_url": "https://github.com/o/r/issues/1",
            "node_id": "I_kwDO",
            "title": "Login bug",
            "body": null,
            "labels": [],
            "user": {"login": "rabin", "avatar_url": "https://x", "followers_url": "https://y"},
            "reactions": {"total_count": 0, "+1": 0}
        });
        let pretty = serde_json::to_string_pretty(&issue).unwrap();
        let out = shape_text(&pretty, 30, &[]).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["title"], "Login bug");
        assert_eq!(v["html_url"], "https://github.com/o/r/issues/1");
        assert_eq!(v["url"], "https://api.github.com/repos/o/r/issues/1");
        assert!(v.get("repository_url").is_none() && v.get("node_id").is_none());
        assert!(v.get("body").is_none() && v.get("labels").is_none());
        assert_eq!(v["user"], json!({"login": "rabin"}));
        assert_eq!(v["reactions"]["total_count"], 0, "zero is data, not noise");
        assert!(out.len() < pretty.len() / 2);
    }

    #[test]
    fn caps_long_arrays_and_ignores_plain_text() {
        let out = shape_text(&json!((0..100).collect::<Vec<_>>()).to_string(), 3, &[]).unwrap();
        assert_eq!(out, r#"[0,1,2,"[mcp-router: 97 more items omitted; narrow the query or paginate]"]"#);
        assert_eq!(shape_text("The sum is 42", 3, &[]), None);
        assert_eq!(shape_text("{not json", 3, &[]), None);
    }

    #[test]
    fn extra_keys_are_dropped() {
        let out = shape_text(r#"{"a":1,"trace":"x"}"#, 3, &["trace".into()]).unwrap();
        assert_eq!(out, r#"{"a":1}"#);
    }
}
