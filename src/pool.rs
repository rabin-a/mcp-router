//! Downstream MCP servers: started lazily on first use, with tool lists cached on disk
//! so `find_tools` works without spawning anything.

use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow};
use http::{HeaderName, HeaderValue};
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, CallToolResult, JsonObject, Tool},
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, OnceCell};

use crate::config::{Config, ServerConfig, cache_dir, expand};

const START_TIMEOUT: Duration = Duration::from_secs(60);

type Client = RunningService<RoleClient, ()>;

#[derive(Serialize, Deserialize)]
struct CacheFile {
    /// Fingerprint of command/args/env; a config change invalidates the cache.
    fingerprint: String,
    tools: Vec<Tool>,
}

pub struct Pool {
    pub config: Config,
    clients: Mutex<HashMap<String, Arc<OnceCell<Arc<Client>>>>>,
    catalog: OnceCell<HashMap<String, Vec<Tool>>>,
}

impl Pool {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            clients: Mutex::default(),
            catalog: OnceCell::new(),
        }
    }

    fn server(&self, name: &str) -> Result<&ServerConfig> {
        self.config
            .servers
            .get(name)
            .ok_or_else(|| anyhow!("unknown server `{name}`"))
    }

    /// Connected client for `name`, spawning the server on first use.
    async fn client(&self, name: &str) -> Result<Arc<Client>> {
        let cell = self
            .clients
            .lock()
            .await
            .entry(name.to_string())
            .or_default()
            .clone();
        let server = self.server(name)?;
        cell.get_or_try_init(|| async {
            let connecting = async {
                match &server.url {
                    Some(url) => {
                        let mut headers = HashMap::new();
                        for (key, value) in &server.headers {
                            headers.insert(
                                HeaderName::try_from(key.as_str())
                                    .with_context(|| format!("bad header name `{key}`"))?,
                                HeaderValue::try_from(expand(value))
                                    .with_context(|| format!("bad value for header `{key}`"))?,
                            );
                        }
                        let config = StreamableHttpClientTransportConfig::with_uri(expand(url))
                            .custom_headers(headers);
                        let transport = StreamableHttpClientTransport::from_config(config);
                        Ok::<_, anyhow::Error>(().serve(transport).await?)
                    }
                    None => {
                        let mut cmd = tokio::process::Command::new(expand(&server.command));
                        cmd.args(server.args.iter().map(|a| expand(a)))
                            .envs(server.env.iter().map(|(k, v)| (k, expand(v))));
                        let transport = TokioChildProcess::new(cmd).with_context(|| {
                            format!("spawning `{}` for server `{name}`", server.command)
                        })?;
                        Ok(().serve(transport).await?)
                    }
                }
            };
            let client = tokio::time::timeout(START_TIMEOUT, connecting)
                .await
                .map_err(|_| anyhow!("server `{name}` did not start within {START_TIMEOUT:?}"))?
                .with_context(|| format!("initializing server `{name}`"))?;
            Ok::<_, anyhow::Error>(Arc::new(client))
        })
        .await
        .cloned()
    }

    /// Every server's tools, from cache where possible. Servers that fail are skipped
    /// with a warning so one broken server never hides the rest.
    pub async fn catalog(&self) -> &HashMap<String, Vec<Tool>> {
        self.catalog
            .get_or_init(|| async {
                let loads = self.config.servers.keys().map(|name| async move {
                    match self.tools_for(name, false).await {
                        Ok(tools) => Some((name.clone(), tools)),
                        Err(e) => {
                            tracing::warn!("skipping server `{name}`: {e:#}");
                            None
                        }
                    }
                });
                futures::future::join_all(loads)
                    .await
                    .into_iter()
                    .flatten()
                    .collect()
            })
            .await
    }

    pub async fn tools_for(&self, name: &str, refresh: bool) -> Result<Vec<Tool>> {
        let server = self.server(name)?;
        let fingerprint = format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}",
            server.command, server.args, server.env, server.url, server.headers
        );
        let path = cache_dir().join(format!("{name}.json"));
        if !refresh
            && let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(cache) = serde_json::from_str::<CacheFile>(&text)
            && cache.fingerprint == fingerprint
        {
            return Ok(cache.tools);
        }
        let tools = self.client(name).await?.list_all_tools().await?;
        std::fs::create_dir_all(cache_dir())?;
        std::fs::write(
            &path,
            serde_json::to_string(&CacheFile {
                fingerprint,
                tools: tools.clone(),
            })?,
        )?;
        Ok(tools)
    }

    pub async fn call(
        &self,
        server: &str,
        tool: &str,
        arguments: JsonObject,
    ) -> Result<CallToolResult> {
        let params = CallToolRequestParams::new(tool.to_string()).with_arguments(arguments);
        Ok(self.client(server).await?.call_tool(params).await?)
    }
}
