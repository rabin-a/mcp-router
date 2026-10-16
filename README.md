# mcp-router

[![ci](https://github.com/rabin-a/mcp-router/actions/workflows/ci.yml/badge.svg)](https://github.com/rabin-a/mcp-router/actions/workflows/ci.yml)

**Website:** https://rabin-a.github.io/mcp-router/

> **Benchmark:** six open-source MCP proxies and mcp-router, measured head-to-head
> in Claude Code. Most proxies *increase* tokens once a tool is called; mcp-router
> cut them 13–44% and finished every task. [Read BENCHMARK.md →](BENCHMARK.md)

One MCP server in front of many. Claude Code loads three tools — `find_tools`,
`call_tool` and `run_script` — instead of every tool from every server. Downstream servers start
only when first used; their tool lists are cached in `~/Library/Caches/mcp-router` (macOS) or `~/.cache/mcp-router` (Linux).

- **Fewer tokens:** ~250 tokens for the router instead of a name (or schema) per
  downstream tool. Schemas are sent only for search hits, with noise keys
  (`$schema`, `title`, long descriptions) stripped.
- **Capped output:** downstream text results are truncated at `max_output_chars`.
- **Deny list:** glob patterns over `server.tool` that are never forwarded or shown.
- **Faster startup:** nothing is spawned until a tool is needed.

## Install

macOS and Linux (x86_64 and arm64):

```sh
curl -fsSL https://raw.githubusercontent.com/rabin-a/mcp-router/master/install.sh | sh
```

This installs to `~/.local/bin` after verifying the SHA-256 checksum. Set
`MCP_ROUTER_INSTALL_DIR` to change the location, or `MCP_ROUTER_VERSION=v0.1.0`
to pin a release. Binaries are also on the
[Releases](https://github.com/rabin-a/mcp-router/releases) page.

From source:

```sh
cargo install --git https://github.com/rabin-a/mcp-router
```

## Move your servers from Claude Code

`import` copies the MCP servers Claude Code already uses in the current directory
(user, project and local scopes) into the router, then prints the
`claude mcp remove` commands that stop Claude loading them directly:

```sh
mcp-router import --dry-run   # preview
mcp-router import
claude mcp add router -s user -- mcp-router
```

## Add servers

Same syntax as `claude mcp add`:

```sh
mcp-router add -e GITHUB_TOKEN=... github -- npx -y @modelcontextprotocol/server-github
mcp-router add -t http -H "Authorization: Bearer ${API_TOKEN}" linear https://mcp.linear.app/mcp
mcp-router add-json weather '{"type":"http","url":"https://api.weather.example/mcp"}'
mcp-router list
mcp-router remove weather
```

Stdio and streamable HTTP servers are supported; SSE isn't. claude.ai connectors
can't be routed, because they're authenticated by claude.ai, not locally.

## Configure

Servers live in `~/.config/mcp-router/servers.toml` (or `$MCP_ROUTER_CONFIG`). You
can edit it by hand too. Values may use `${VAR}` and `${VAR:-default}`, as in `.mcp.json`:

```toml
max_output_chars = 20000
deny = ["*.delete_*", "gmail.send*"]

[servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "${GITHUB_TOKEN}" }
description = "GitHub repos, issues, PRs"

[servers.linear]
url = "https://mcp.linear.app/mcp"
headers = { Authorization = "Bearer ${LINEAR_TOKEN}" }
```

## Commands

```sh
mcp-router                 # serve over stdio (what Claude Code runs)
mcp-router import          # copy servers from Claude Code
mcp-router add | add-json | remove | list
mcp-router refresh [name]  # re-fetch tool lists into the cache
mcp-router search <query>  # try a search from the shell
mcp-router stats           # tools per server, schema tokens kept out of context
```

## Response shaping

JSON results are minified and stripped of noise before they reach Claude:
`*_url` link templates (except `html_url`), `node_id`, `avatar_url`, nulls,
empty strings/arrays/objects, and arrays beyond `max_array_items` (default 50).
Turn it off with `shape_json = false`; add keys with `drop_keys = ["trace"]`.

## Code mode

`run_script` runs a JavaScript function body in an embedded QuickJS sandbox (no
filesystem, network, or imports; 60s, 50 tool calls, 64 MB). `call("server.tool", args)`
returns the parsed result and throws on error; only the `return` value and
`console.log` output reach Claude. Calls go through the deny list. Turn it off
with `code_mode = false`.

## Benchmarks

Headless Claude Code (`claude -p`, Sonnet), total input tokens per task, mean of 2 runs:

| Setup | Direct MCP | Router |
|---|---|---|
| 480 tools, no MCP call needed | 34.3k | 28.5k (−17%) |
| 480 tools, one MCP call | 104k | 88k (−15%) |
| GitHub list of 20 issues (55 KB JSON) | 176k, 5 turns | 95k, 3 turns (−46%) |
| 37 tools, one MCP call | 89–121k | −15% to +12% (no clear win) |
| GitHub list → 6 Jira creates | 241k, 11 turns, 29s | 177k (−27%); with `run_script`: 6 turns, 14s |

The GitHub win comes from shaping: the raw result passes Claude Code's
50,000-character limit and is saved to a file Claude must read back.

## Trade-offs

- Claude Code's per-tool permission prompts collapse into one `call_tool`
  prompt; use `deny` for anything you never want called.
- One extra round trip (`find_tools`) the first time a tool is used.
- Search is keyword BM25. If it misses, `find_tools` with only `server` lists
  that server's tools.
