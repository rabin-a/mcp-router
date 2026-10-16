mod config;
mod pool;
mod router;
mod search;
mod shape;

use anyhow::Result;
use rmcp::{ServiceExt, transport::stdio};

use crate::{config::Config, pool::Pool, router::Router};

const USAGE: &str = "\
mcp-router — one MCP server in front of many, so Claude loads 2 tools instead of hundreds.

usage:
  mcp-router                 serve over stdio (what Claude Code runs)
  mcp-router refresh [name]  re-fetch tool lists into the cache
  mcp-router search <query>  try a search from the shell
  mcp-router stats           tools per server and estimated schema tokens saved

config: $MCP_ROUTER_CONFIG or ~/.config/mcp-router/servers.toml";

#[tokio::main]
async fn main() -> Result<()> {
    // stdout is the MCP channel; logs go to stderr.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let pool = Pool::new(Config::load()?);
    match args.first().map(String::as_str) {
        None | Some("serve") => {
            let service = Router::new(pool).serve(stdio()).await?;
            service.waiting().await?;
        }
        Some("refresh") => {
            let names: Vec<String> = match args.get(1) {
                Some(name) => vec![name.clone()],
                None => pool.config.servers.keys().cloned().collect(),
            };
            for name in names {
                match pool.tools_for(&name, true).await {
                    Ok(tools) => println!("{name}: {} tools", tools.len()),
                    Err(e) => println!("{name}: FAILED {e:#}"),
                }
            }
        }
        Some("search") => {
            let query = args[1..].join(" ");
            let catalog = pool.catalog().await;
            let docs = catalog
                .iter()
                .flat_map(|(s, tools)| {
                    tools.iter().map(|t| search::Doc {
                        server: s.clone(),
                        tool: t.name.to_string(),
                        description: t.description.as_deref().unwrap_or_default().to_string(),
                    })
                })
                .collect();
            for d in search::Index::new(docs).search(&query, 10) {
                println!("{}.{}", d.server, d.tool);
            }
        }
        Some("stats") => {
            let mut total_chars = 0;
            for (name, tools) in pool.catalog().await {
                let chars: usize = tools.iter().map(|t| serde_json::to_string(t).map_or(0, |s| s.len())).sum();
                total_chars += chars;
                println!("{name}: {} tools, ~{} schema tokens", tools.len(), chars / 4);
            }
            println!("total: ~{} tokens of schemas kept out of context (router itself: ~250)", total_chars / 4);
        }
        Some(_) => println!("{USAGE}"),
    }
    Ok(())
}
