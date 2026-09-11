//! MongoDB MCP server — a Rust reimplementation of the Node reference server.

mod config;
mod registry;
mod server;
mod session;
mod tools;

use std::sync::Arc;

use anyhow::{Context as _, Result};
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // stdout carries the MCP stream, so logs go to stderr.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("MDB_MCP_LOG").unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let cfg = Arc::new(config::Config::from_env_and_args(std::env::args().skip(1))?);
    let registry = Arc::new(registry::Registry::load(&cfg).context("loading the tool registry")?);
    let connections = Arc::new(session::Connections::new(&cfg));

    // A configured connection string means the caller should not have to call
    // connect before doing anything.
    if let Some(uri) = &cfg.connection_string {
        match connections.connect(uri, Some("default")).await {
            Ok(c) => tracing::info!(connection = %c.redacted_uri, "connected at startup"),
            Err(e) => tracing::warn!("could not connect at startup: {e}"),
        }
    }

    let ctx = Arc::new(tools::Context { connections, config: cfg.clone() });
    tracing::info!(tools = registry.visible().len(), read_only = cfg.read_only, "mongodb-mcp starting");

    let service = server::MongoMcp::new(registry, ctx)
        .serve(stdio())
        .await
        .context("starting the MCP stdio transport")?;
    service.waiting().await?;
    Ok(())
}
