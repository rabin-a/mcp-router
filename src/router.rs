//! The MCP server Claude talks to: two tools, `find_tools` and `call_tool`.

use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, JsonObject,
        ListToolsResult, PaginatedRequestParams, ResultType, ServerCapabilities, ServerConfig,
        Tool,
    },
    service::RequestContext,
};
use serde_json::{Value, json};

use crate::{
    config::Config,
    pool::Pool,
    sandbox::{self, Limits, ToolCall},
    search::{Doc, Index},
    shape::shape_text,
};

const DEFAULT_LIMIT: usize = 5;
const MAX_DESCRIPTION: usize = 160;

#[derive(Clone)]
pub struct Router {
    pool: Arc<Pool>,
    index: Arc<tokio::sync::OnceCell<Index>>,
}

impl Router {
    pub fn new(pool: Pool) -> Self {
        Self { pool: Arc::new(pool), index: Arc::default() }
    }

    async fn index(&self) -> &Index {
        self.index
            .get_or_init(|| async {
                let docs = self
                    .pool
                    .catalog()
                    .await
                    .iter()
                    .flat_map(|(server, tools)| {
                        tools.iter().map(|t| Doc {
                            server: server.clone(),
                            tool: t.name.to_string(),
                            description: t.description.as_deref().unwrap_or_default().to_string(),
                        })
                    })
                    .collect();
                Index::new(docs)
            })
            .await
    }

    async fn find_tools(&self, args: &JsonObject) -> CallToolResult {
        let query = args.get("query").and_then(Value::as_str).unwrap_or_default();
        let server = args.get("server").and_then(Value::as_str);
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map_or(DEFAULT_LIMIT, |n| n.clamp(1, 20) as usize);
        let catalog = self.pool.catalog().await;

        // `server` with no query: list that server's tool names, no schemas.
        if let (Some(server), true) = (server, query.trim().is_empty()) {
            let Some(tools) = catalog.get(server) else {
                return error(format!("unknown server `{server}`"));
            };
            let lines: Vec<String> = tools
                .iter()
                .filter(|t| !self.pool.config.is_denied(&format!("{server}.{}", t.name)))
                .map(|t| format!("{server}.{} — {}", t.name, short(t.description.as_deref())))
                .collect();
            return text(lines.join("\n"));
        }

        let index = self.index().await;
        let hits: Vec<&Doc> = index
            .search(query, limit * 4)
            .into_iter()
            .filter(|d| server.is_none_or(|s| d.server == s))
            .filter(|d| !self.pool.config.is_denied(&format!("{}.{}", d.server, d.tool)))
            .take(limit)
            .collect();
        if hits.is_empty() {
            let servers: Vec<&String> = catalog.keys().collect();
            return text(format!(
                "No tools matched `{query}` among {} tools. Try other words, or pass `server` alone to list one server's tools. Servers: {servers:?}",
                index.len()
            ));
        }

        let entries: Vec<String> = hits
            .iter()
            .filter_map(|d| {
                let tool = catalog.get(&d.server)?.iter().find(|t| t.name == d.tool)?;
                let schema = compact_schema(Value::Object((*tool.input_schema).clone()));
                Some(format!(
                    "{}.{} — {}\n  args: {}",
                    d.server,
                    d.tool,
                    short(tool.description.as_deref()),
                    schema
                ))
            })
            .collect();
        text(format!(
            "{}\nCall call_tool now with the best match; no need to search again.",
            entries.join("\n")
        ))
    }

    async fn call_tool_downstream(&self, args: &JsonObject) -> CallToolResult {
        let Some(qualified) = args.get("tool").and_then(Value::as_str) else {
            return error("`tool` is required, as `server.tool`".into());
        };
        let Some((server, tool)) = qualified.split_once('.') else {
            return error(format!("`{qualified}` must be `server.tool`; use find_tools to get the name"));
        };
        if self.pool.config.is_denied(qualified) {
            return error(format!("`{qualified}` is blocked by the router's deny list"));
        }
        let arguments = match args.get("arguments") {
            Some(Value::Object(map)) => map.clone(),
            None | Some(Value::Null) => JsonObject::new(),
            Some(_) => return error("`arguments` must be an object".into()),
        };
        match self.pool.call(server, tool, arguments).await {
            Ok(result) => trim_result(result, &self.pool.config),
            Err(e) => error(format!("{qualified} failed: {e:#}")),
        }
    }

    async fn run_script(&self, args: &JsonObject) -> CallToolResult {
        let Some(code) = args.get("code").and_then(Value::as_str) else {
            return error("`code` is required".into());
        };
        let code = code.to_string();
        let pool = self.pool.clone();
        let handle = tokio::runtime::Handle::current();
        // QuickJS is synchronous; tool calls inside the script block this thread on the runtime.
        let outcome = tokio::task::spawn_blocking(move || {
            let call: ToolCall = Box::new(move |qualified, args| {
                let (server, tool) = qualified
                    .split_once('.')
                    .ok_or_else(|| format!("`{qualified}` must be `server.tool`"))?;
                if pool.config.is_denied(qualified) {
                    return Err(format!("`{qualified}` is blocked by the router's deny list"));
                }
                let Value::Object(args) = args else {
                    return Err("arguments must be an object".into());
                };
                let result = handle.block_on(pool.call(server, tool, args)).map_err(|e| format!("{e:#}"))?;
                tool_value(result)
            });
            sandbox::run(&code, call, &Limits::default())
        })
        .await;
        match outcome {
            Ok(Ok(value)) => trim_result(text(value.to_string()), &self.pool.config),
            Ok(Err(e)) => error(e),
            Err(e) => error(format!("script crashed: {e}")),
        }
    }
}

/// A downstream result as the script sees it: parsed JSON when the text is JSON.
fn tool_value(result: CallToolResult) -> Result<Value, String> {
    let text: Vec<&str> = result
        .content
        .iter()
        .filter_map(|c| match c {
            ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect();
    let text = text.join("\n");
    if result.is_error == Some(true) {
        return Err(text);
    }
    if text.is_empty()
        && let Some(structured) = result.structured_content
    {
        return Ok(structured);
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

impl ServerHandler for Router {
    fn get_info(&self) -> ServerConfig {
        let servers: Vec<String> = self
            .pool
            .config
            .servers
            .iter()
            .map(|(name, s)| {
                if s.description.is_empty() { name.clone() } else { format!("{name} ({})", s.description) }
            })
            .collect();
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(format!(
            "Routes to these MCP servers: {}. Call find_tools with what you want to do, then call_tool with the returned `server.tool` name.{}",
            servers.join(", "),
            if self.pool.config.code_mode {
                " For several dependent calls, or a big result you only need part of, use run_script."
            } else {
                ""
            }
        ))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(router_tools(self.pool.config.code_mode)))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = request.arguments.unwrap_or_default();
        let result = match request.name.as_ref() {
            "find_tools" => self.find_tools(&args).await,
            "call_tool" => self.call_tool_downstream(&args).await,
            "run_script" if self.pool.config.code_mode => self.run_script(&args).await,
            other => error(format!("unknown tool `{other}`")),
        };
        Ok(result.into())
    }
}

fn router_tools(code_mode: bool) -> Vec<Tool> {
    let schema = |v: Value| match v {
        Value::Object(map) => map,
        _ => unreachable!(),
    };
    // Claude Code defers MCP tools behind its own ToolSearch; deferring the router's
    // own tools would cost an extra turn before every routed call.
    let always_load = |mut tool: Tool| {
        tool.meta = serde_json::from_value(json!({"anthropic/alwaysLoad": true})).ok();
        tool
    };
    [
        Tool::new(
            "find_tools",
            "Search tools on the connected MCP servers. Returns `server.tool` names with argument schemas.",
            schema(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "What you want to do, in a few words"},
                    "server": {"type": "string", "description": "Limit to one server; with no query, lists its tools"},
                    "limit": {"type": "integer", "description": "Max results (default 5)"}
                }
            })),
        ),
        Tool::new(
            "call_tool",
            "Call a tool found with find_tools.",
            schema(json!({
                "type": "object",
                "properties": {
                    "tool": {"type": "string", "description": "`server.tool` from find_tools"},
                    "arguments": {"type": "object", "description": "Arguments matching the tool's schema"}
                },
                "required": ["tool"]
            })),
        ),
        Tool::new(
            "run_script",
            "Run JavaScript that calls tools, for multi-step work or large results you only need part of. \
             `call(\"server.tool\", args)` returns the tool's result (parsed JSON when possible) and throws on error. \
             `return` the value you want; only it (plus console.log output) comes back. \
             No filesystem, network or imports; 60s and 50 calls max. Get names and schemas from find_tools first.",
            schema(json!({
                "type": "object",
                "properties": {
                    "code": {"type": "string", "description": "Function body, e.g. const r = call(\"github.list_issues\", {repo: \"o/r\"}); return r.length;"}
                },
                "required": ["code"]
            })),
        ),
    ]
    .into_iter()
    .filter(|t| code_mode || t.name != "run_script")
    .map(always_load)
    .collect()
}

fn text(s: String) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(s)])
}

fn error(s: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(s)])
}

fn short(description: Option<&str>) -> String {
    let first_line = description.unwrap_or_default().lines().next().unwrap_or_default().trim();
    clip(first_line, MAX_DESCRIPTION)
}

fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((idx, _)) => format!("{}…", &s[..idx]),
        None => s.to_string(),
    }
}

/// Drop schema keys that cost tokens without helping the model call the tool.
fn compact_schema(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(k, v)| {
                    !(matches!(k.as_str(), "$schema" | "title" | "examples" | "$id")
                        || k == "additionalProperties" && v == &Value::Bool(false))
                })
                .map(|(k, v)| match (k.as_str(), v) {
                    ("description", Value::String(s)) => (k, Value::String(clip(&s, MAX_DESCRIPTION))),
                    // Keys under `properties` are argument names, not schema keywords: keep them all.
                    ("properties", Value::Object(props)) => {
                        let props = props.into_iter().map(|(name, s)| (name, compact_schema(s))).collect();
                        (k, Value::Object(props))
                    }
                    (_, v) => (k, compact_schema(v)),
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(compact_schema).collect()),
        other => other,
    }
}

/// Shape JSON text, then cap the total; non-text content passes through.
fn trim_result(mut result: CallToolResult, config: &Config) -> CallToolResult {
    // Downstream servers on older protocol revisions omit `resultType`; clients on
    // 2026-07-28 reject results without it. rmcp strips it again for older clients.
    result.result_type = Some(ResultType::COMPLETE);
    let mut budget = config.max_output_chars;
    for block in &mut result.content {
        if let ContentBlock::Text(t) = block {
            if config.shape_json
                && let Some(shaped) = shape_text(&t.text, config.max_array_items, &config.drop_keys)
            {
                t.text = shaped;
            }
            let len = t.text.chars().count();
            if len > budget {
                let kept = clip(&t.text, budget);
                t.text = format!("{kept}\n[mcp-router: truncated {} of {len} chars]", len - budget);
                budget = 0;
            } else {
                budget -= len;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compacts_schema() {
        let schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "q": {"type": "string", "title": "Q", "description": "x".repeat(500)},
                "title": {"type": "string"}
            }
        });
        let out = compact_schema(schema);
        assert_eq!(out["properties"]["title"]["type"], "string");
        assert!(out.get("$schema").is_none());
        assert!(out.get("additionalProperties").is_none());
        let desc = out["properties"]["q"]["description"].as_str().unwrap();
        assert_eq!(desc.chars().count(), MAX_DESCRIPTION + 1);
        assert!(out["properties"]["q"].get("title").is_none());
    }

    #[test]
    fn truncates_text() {
        let mut result = CallToolResult::success(vec![ContentBlock::text("a".repeat(100))]);
        result.result_type = None; // as deserialized from an older-protocol server
        let config: Config = toml::from_str("max_output_chars = 10").unwrap();
        let out = trim_result(result, &config);
        assert_eq!(out.result_type, Some(ResultType::COMPLETE));
        let ContentBlock::Text(t) = &out.content[0] else { panic!() };
        assert!(t.text.starts_with("aaaaaaaaaa…"));
        assert!(t.text.contains("truncated 90 of 100"));
    }
}
