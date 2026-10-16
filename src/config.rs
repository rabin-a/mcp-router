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

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// One line shown to the model so it knows what the server is for.
    #[serde(default)]
    pub description: String,
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
    use super::glob_match;

    #[test]
    fn globs() {
        assert!(glob_match("*.delete_*", "github.delete_repo"));
        assert!(!glob_match("*.delete_*", "github.list_repos"));
        assert!(glob_match("gmail.*", "gmail.send"));
        assert!(glob_match("exact.tool", "exact.tool"));
        assert!(!glob_match("a*c", "abd"));
    }
}
