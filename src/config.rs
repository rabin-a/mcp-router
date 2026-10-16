use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    /// Max characters of text returned from a downstream tool call.
    #[serde(default = "default_max_output")]
    pub max_output_chars: usize,
    /// Glob patterns over `server.tool` that are never forwarded, e.g. "*.delete_*".
    #[serde(default)]
    pub deny: Vec<String>,
    /// Strip noise (link templates, nulls, empties, whitespace) from JSON results.
    #[serde(default = "default_true")]
    pub shape_json: bool,
    /// Arrays in JSON results are cut to this many items.
    #[serde(default = "default_max_items")]
    pub max_array_items: usize,
    /// Extra JSON keys to drop from results, on top of the built-in noise keys.
    #[serde(default)]
    pub drop_keys: Vec<String>,
    /// Offer `run_script` (sandboxed JavaScript that chains tool calls).
    #[serde(default = "default_true")]
    pub code_mode: bool,
    #[serde(default)]
    pub servers: BTreeMap<String, ServerConfig>,
}

/// One downstream server: a stdio `command`, or a streamable-HTTP `url`.
/// Values may use `${VAR}` and `${VAR:-default}`, expanded when the server starts.
#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// One line shown to the model so it knows what the server is for.
    #[serde(default)]
    pub description: String,
}

/// Expand `${VAR}` and `${VAR:-default}` like Claude Code does in `.mcp.json`.
/// An unset variable with no default is left as written.
pub fn expand(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = &after[..end];
        let (name, default) = match inner.split_once(":-") {
            Some((name, default)) => (name, Some(default)),
            None => (inner, None),
        };
        match (std::env::var(name).ok().filter(|v| !v.is_empty()), default) {
            (Some(value), _) => out.push_str(&value),
            (None, Some(default)) => out.push_str(default),
            (None, None) => out.push_str(&rest[start..start + 3 + end]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn default_max_output() -> usize {
    20_000
}

fn default_true() -> bool {
    true
}

fn default_max_items() -> usize {
    50
}

/// `$MCP_ROUTER_CONFIG`, else `$XDG_CONFIG_HOME/mcp-router/servers.toml`, else
/// `~/.config/mcp-router/servers.toml` — on macOS too, where `dirs::config_dir()`
/// would point into `~/Library/Application Support`.
pub fn config_path() -> PathBuf {
    std::env::var_os("MCP_ROUTER_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
                .unwrap_or_else(|| PathBuf::from("."))
                .join("mcp-router/servers.toml")
        })
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mcp-router")
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path();
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn is_denied(&self, qualified: &str) -> bool {
        self.deny
            .iter()
            .any(|pattern| glob_match(pattern, qualified))
    }
}

/// Minimal `*` wildcard matching; enough for deny lists.
fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(idx) => rest = &rest[idx + part.len()..],
                None => return false,
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{expand, glob_match};

    #[test]
    fn expands_env_vars() {
        // SAFETY: tests in this module don't read MCP_ROUTER_TEST_* concurrently.
        unsafe { std::env::set_var("MCP_ROUTER_TEST_TOKEN", "abc") };
        assert_eq!(expand("Bearer ${MCP_ROUTER_TEST_TOKEN}"), "Bearer abc");
        assert_eq!(
            expand("${MCP_ROUTER_TEST_UNSET:-https://x}/mcp"),
            "https://x/mcp"
        );
        assert_eq!(
            expand("${MCP_ROUTER_TEST_UNSET}"),
            "${MCP_ROUTER_TEST_UNSET}"
        );
        assert_eq!(expand("no vars, ${unclosed"), "no vars, ${unclosed");
    }

    #[test]
    fn globs() {
        assert!(glob_match("*.delete_*", "github.delete_repo"));
        assert!(!glob_match("*.delete_*", "github.list_repos"));
        assert!(glob_match("gmail.*", "gmail.send"));
        assert!(glob_match("exact.tool", "exact.tool"));
        assert!(!glob_match("a*c", "abd"));
    }
}
