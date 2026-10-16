//! Editing servers.toml from the command line, with the same syntax as `claude mcp`,
//! and importing the servers already configured in Claude Code.

use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use toml_edit::{DocumentMut, InlineTable, Item, Table, value};

use crate::config::config_path;

/// A server entry as written to servers.toml.
#[derive(Debug, Default, PartialEq)]
pub struct Entry {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub url: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub description: String,
}

impl Entry {
    /// From a Claude Code `mcpServers` value (`.mcp.json`, `~/.claude.json`, `add-json`).
    pub fn from_claude_json(v: &Value) -> Result<Self> {
        let strings = |key: &str| -> BTreeMap<String, String> {
            v.get(key)
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                        .collect()
                })
                .unwrap_or_default()
        };
        match v.get("type").and_then(Value::as_str).unwrap_or("stdio") {
            "stdio" => Ok(Entry {
                command: v
                    .get("command")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("stdio server has no `command`"))?
                    .to_string(),
                args: v
                    .get("args")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                env: strings("env"),
                ..Entry::default()
            }),
            "http" => Ok(Entry {
                url: Some(
                    v.get("url")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow!("http server has no `url`"))?
                        .to_string(),
                ),
                headers: strings("headers"),
                ..Entry::default()
            }),
            other => bail!("`{other}` transport isn't supported (stdio and http are)"),
        }
    }

    fn summary(&self) -> String {
        match &self.url {
            Some(url) => format!("http  {url}"),
            None => format!("stdio {} {}", self.command, self.args.join(" "))
                .trim_end()
                .into(),
        }
    }

    /// The router itself registered in Claude: importing it would make it spawn itself.
    fn is_router(&self) -> bool {
        Path::new(&self.command)
            .file_name()
            .is_some_and(|f| f == "mcp-router")
    }
}

/// Server names become the `server.` prefix in `server.tool`, so they can't contain dots.
fn clean_name(name: &str) -> String {
    name.replace('.', "-")
}

fn load() -> Result<DocumentMut> {
    let path = config_path();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    text.parse()
        .with_context(|| format!("parsing {}", path.display()))
}

fn save(doc: &DocumentMut) -> Result<()> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, doc.to_string()).with_context(|| format!("writing {}", path.display()))
}

fn servers(doc: &mut DocumentMut) -> &mut Table {
    let item = doc.entry("servers").or_insert_with(|| {
        let mut t = Table::new();
        t.set_implicit(true);
        Item::Table(t)
    });
    item.as_table_mut().expect("`servers` must be a table")
}

fn inline(map: &BTreeMap<String, String>) -> InlineTable {
    map.iter()
        .map(|(k, v)| (k.as_str(), toml_edit::Value::from(v.as_str())))
        .collect()
}

fn put(doc: &mut DocumentMut, name: &str, entry: &Entry) {
    let mut t = Table::new();
    match &entry.url {
        Some(url) => {
            t["url"] = value(url.as_str());
            if !entry.headers.is_empty() {
                t["headers"] = value(inline(&entry.headers));
            }
        }
        None => {
            t["command"] = value(entry.command.as_str());
            if !entry.args.is_empty() {
                t["args"] = value(
                    entry
                        .args
                        .iter()
                        .map(String::as_str)
                        .collect::<toml_edit::Array>(),
                );
            }
            if !entry.env.is_empty() {
                t["env"] = value(inline(&entry.env));
            }
        }
    }
    if !entry.description.is_empty() {
        t["description"] = value(entry.description.as_str());
    }
    servers(doc).insert(name, Item::Table(t));
}

/// `mcp-router add [options] <name> -- <command> [args...]`
/// `mcp-router add -t http [options] <name> <url>`
/// Options: -e/--env KEY=value, -H/--header "Name: value", -t/--transport stdio|http,
/// -d/--description TEXT, -s/--scope (accepted for compatibility, ignored), --force.
pub fn parse_add(args: &[String]) -> Result<(String, Entry, bool)> {
    let mut entry = Entry::default();
    let mut transport = "stdio".to_string();
    let mut positional = Vec::new();
    let mut force = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let (flag, inline_value) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut take = |what: &str| -> Result<String> {
            inline_value
                .clone()
                .or_else(|| it.next().cloned())
                .ok_or_else(|| anyhow!("{flag} needs {what}"))
        };
        match flag {
            "--" => {
                positional.extend(it.by_ref().cloned());
                break;
            }
            "-e" | "--env" => {
                let kv = take("KEY=value")?;
                let (k, v) = kv
                    .split_once('=')
                    .ok_or_else(|| anyhow!("--env expects KEY=value, got `{kv}`"))?;
                entry.env.insert(k.into(), v.into());
            }
            "-H" | "--header" => {
                let h = take("\"Name: value\"")?;
                let (k, v) = h
                    .split_once(':')
                    .ok_or_else(|| anyhow!("--header expects \"Name: value\", got `{h}`"))?;
                entry.headers.insert(k.trim().into(), v.trim().into());
            }
            "-t" | "--transport" => transport = take("stdio or http")?,
            "-d" | "--description" => entry.description = take("text")?,
            "-s" | "--scope" => {
                take("a scope")?;
            }
            "-f" | "--force" => force = true,
            f if f.starts_with('-') && positional.is_empty() => bail!("unknown option `{f}`"),
            _ => positional.push(arg.clone()),
        }
    }
    let mut positional = positional.into_iter();
    let name = clean_name(
        &positional
            .next()
            .ok_or_else(|| anyhow!("missing server name"))?,
    );
    match transport.as_str() {
        "stdio" => {
            entry.command = positional.next().ok_or_else(|| {
                anyhow!("missing command; use: mcp-router add <name> -- <command> [args...]")
            })?;
            entry.args = positional.collect();
            if !entry.headers.is_empty() {
                bail!("--header only applies to http servers");
            }
        }
        "http" => {
            entry.url =
                Some(positional.next().ok_or_else(|| {
                    anyhow!("missing URL; use: mcp-router add -t http <name> <url>")
                })?);
            if positional.next().is_some() {
                bail!("unexpected arguments after the URL");
            }
            if !entry.env.is_empty() {
                bail!("--env only applies to stdio servers");
            }
        }
        "sse" => bail!("SSE servers aren't supported; most now offer streamable HTTP (-t http)"),
        other => bail!("unknown transport `{other}`; use stdio or http"),
    }
    Ok((name, entry, force))
}

pub fn add(name: &str, entry: &Entry, force: bool) -> Result<()> {
    let mut doc = load()?;
    if servers(&mut doc).contains_key(name) && !force {
        bail!("server `{name}` already exists; pass --force to replace it");
    }
    put(&mut doc, name, entry);
    save(&doc)?;
    println!(
        "Added `{name}` ({}) to {}",
        entry.summary(),
        config_path().display()
    );
    Ok(())
}

pub fn add_json(name: &str, json: &str, force: bool) -> Result<()> {
    let value: Value = serde_json::from_str(json).context("parsing JSON")?;
    add(&clean_name(name), &Entry::from_claude_json(&value)?, force)
}

pub fn remove(name: &str) -> Result<()> {
    let mut doc = load()?;
    if servers(&mut doc).remove(name).is_none() {
        bail!("no server named `{name}`");
    }
    save(&doc)?;
    println!("Removed `{name}`");
    Ok(())
}

pub fn list() -> Result<()> {
    let config = crate::config::Config::load()?;
    if config.servers.is_empty() {
        println!("No servers yet. Add one with `mcp-router add`, or run `mcp-router import`.");
    }
    for (name, s) in &config.servers {
        let target = match &s.url {
            Some(url) => format!("http  {url}"),
            None => format!("stdio {} {}", s.command, s.args.join(" ")),
        };
        println!("{name:<20} {}", target.trim_end());
    }
    Ok(())
}

/// Servers configured in Claude Code for `project`, highest precedence last:
/// user (`~/.claude.json`), project (`.mcp.json`), local (`~/.claude.json` under the path).
fn claude_servers(project: &Path) -> Result<Vec<(&'static str, String, Value)>> {
    let mut found = Vec::new();
    let mut collect = |scope: &'static str, servers: Option<&Value>| {
        if let Some(map) = servers.and_then(Value::as_object) {
            found.extend(map.iter().map(|(n, v)| (scope, n.clone(), v.clone())));
        }
    };
    let home = dirs::home_dir().ok_or_else(|| anyhow!("no home directory"))?;
    let claude: Option<Value> = std::fs::read_to_string(home.join(".claude.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let mcp_json: Option<Value> = std::fs::read_to_string(project.join(".mcp.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    collect("user", claude.as_ref().and_then(|c| c.get("mcpServers")));
    collect(
        "project",
        mcp_json.as_ref().and_then(|c| c.get("mcpServers")),
    );
    let key = project.to_string_lossy();
    collect(
        "local",
        claude
            .as_ref()
            .and_then(|c| c.get("projects"))
            .and_then(|p| p.get(key.as_ref()))
            .and_then(|p| p.get("mcpServers")),
    );
    Ok(found)
}

/// `mcp-router import [--dry-run] [--force]`: copy Claude Code's MCP servers for the
/// current directory into servers.toml, then print how to stop Claude loading them directly.
pub fn import(args: &[String]) -> Result<()> {
    let dry_run = args.iter().any(|a| a == "--dry-run" || a == "-n");
    let force = args.iter().any(|a| a == "--force" || a == "-f");
    let project = std::env::current_dir()?;
    let mut doc = load()?;
    let mut imported = Vec::new();
    // Later scopes override earlier ones, matching Claude Code's precedence.
    let mut chosen: BTreeMap<String, (&str, String, Value)> = BTreeMap::new();
    for (scope, name, v) in claude_servers(&project)? {
        chosen.insert(clean_name(&name), (scope, name, v));
    }
    if chosen.is_empty() {
        println!(
            "No MCP servers found in ~/.claude.json or ./.mcp.json for {}.",
            project.display()
        );
        return Ok(());
    }
    for (name, (scope, original, v)) in &chosen {
        let entry = match Entry::from_claude_json(v) {
            Ok(e) if e.is_router() => {
                println!("skip  {name:<20} this is mcp-router itself");
                continue;
            }
            Ok(e) => e,
            Err(e) => {
                println!("skip  {name:<20} {e}");
                continue;
            }
        };
        if servers(&mut doc).contains_key(name) && !force {
            println!("skip  {name:<20} already in servers.toml (use --force to replace)");
            continue;
        }
        println!("add   {name:<20} {} [{scope}]", entry.summary());
        put(&mut doc, name, &entry);
        imported.push((*scope, original.clone()));
    }
    if dry_run {
        println!("\nDry run: nothing written.");
        return Ok(());
    }
    if imported.is_empty() {
        return Ok(());
    }
    save(&doc)?;
    println!(
        "\nImported {} server(s) into {}.",
        imported.len(),
        config_path().display()
    );
    println!(
        "\nClaude Code still loads them directly. To route them through mcp-router instead, run:"
    );
    for (scope, name) in &imported {
        println!("  claude mcp remove {name} -s {scope}");
    }
    if !chosen
        .values()
        .any(|(_, _, v)| Entry::from_claude_json(v).is_ok_and(|e| e.is_router()))
    {
        println!("  claude mcp add router -s user -- mcp-router");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parses_claude_style_stdio_add() {
        let (name, e, force) = parse_add(&args(
            "--env KEY=abc -e B=2 -s user github -- npx -y @mcp/server-github --port 8080",
        ))
        .unwrap();
        assert_eq!(name, "github");
        assert!(!force);
        assert_eq!(e.command, "npx");
        assert_eq!(e.args, args("-y @mcp/server-github --port 8080"));
        assert_eq!(e.env["KEY"], "abc");
        assert_eq!(e.env["B"], "2");
    }

    #[test]
    fn parses_http_add_with_headers() {
        let raw = vec![
            "--transport".into(),
            "http".into(),
            "api".into(),
            "https://x/mcp".into(),
            "--header".into(),
            "Authorization: Bearer ${TOKEN}".into(),
        ];
        let (name, e, _) = parse_add(&raw).unwrap();
        assert_eq!(name, "api");
        assert_eq!(e.url.as_deref(), Some("https://x/mcp"));
        assert_eq!(e.headers["Authorization"], "Bearer ${TOKEN}");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_add(&args("-t sse x https://y")).is_err());
        assert!(parse_add(&args("x")).is_err());
        assert!(parse_add(&args("-t http x")).is_err());
        assert!(parse_add(&args("--bogus x -- cmd")).is_err());
    }

    #[test]
    fn converts_claude_json() {
        let stdio = Entry::from_claude_json(
            &json!({"command": "npx", "args": ["-y", "pkg"], "env": {"K": "v"}}),
        )
        .unwrap();
        assert_eq!(
            (
                stdio.command.as_str(),
                stdio.args.len(),
                stdio.env["K"].as_str()
            ),
            ("npx", 2, "v")
        );
        let http = Entry::from_claude_json(
            &json!({"type": "http", "url": "https://x", "headers": {"A": "b"}}),
        )
        .unwrap();
        assert_eq!(http.url.as_deref(), Some("https://x"));
        assert!(Entry::from_claude_json(&json!({"type": "sse", "url": "https://x"})).is_err());
        assert!(
            Entry::from_claude_json(&json!({"command": "/usr/local/bin/mcp-router"}))
                .unwrap()
                .is_router()
        );
    }

    #[test]
    fn writes_toml_that_loads_back() {
        let mut doc = DocumentMut::new();
        let (_, e, _) = parse_add(&args("-e K=v -d Repos gh -- npx -y pkg")).unwrap();
        put(&mut doc, "gh", &e);
        put(
            &mut doc,
            "api",
            &Entry {
                url: Some("https://x".into()),
                ..Entry::default()
            },
        );
        let config: crate::config::Config = toml::from_str(&doc.to_string()).unwrap();
        assert_eq!(config.servers["gh"].args, args("-y pkg"));
        assert_eq!(config.servers["gh"].env["K"], "v");
        assert_eq!(config.servers["gh"].description, "Repos");
        assert_eq!(config.servers["api"].url.as_deref(), Some("https://x"));
        assert_eq!(clean_name("my.server"), "my-server");
    }
}
